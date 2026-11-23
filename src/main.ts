import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Phase = "idle" | "recording" | "processing" | "done" | "error";
interface StateEvent { phase: Phase; message?: string }

const pill = document.getElementById("pill") as HTMLDivElement;
const msg = document.getElementById("msg") as HTMLDivElement;
const wave = document.getElementById("wave") as HTMLDivElement;
const strip = document.getElementById("strip") as HTMLDivElement;

// The wave is plain elements, not a <canvas>. On macOS a canvas in this window could keep showing
// an old frame (the last take's orange "processing" wave) after the window was hidden and shown
// again, while the script went on drawing into it unseen. Elements go through the same path as
// the rest of the pill, which always repaints.
//
// Every bar moves by `transform` only, so the page is never laid out or repainted while the wave
// runs; only GPU layers move. While recording, the bars sit on a strip that scrolls left at a
// steady one bar per level, run by a single animation the compositor plays by itself, so no
// script runs per frame. Each level that arrives sets the height of one bar just out of sight on
// the right, which the scroll then brings in. While processing and when done, the bars run on
// CSS animations (index.html). MEASURED in a WKWebView with real levels: about a quarter of the
// CPU of the old per-frame script (25 against 100 ms per second across WebKit's processes), and
// the scroll no longer steps once per level.

/** Bars visible across the wave. */
const BARS = 32;
/** Bars in the ring: the visible ones, plus room to wait out of sight on the right. */
const RING = BARS + 14;
/** The app sends a level this often (audio.rs, `LEVELS_PER_SECOND`). */
const LEVEL_MS = 1000 / 30;
/** How many levels the scroll runs behind the newest, which absorbs uneven delivery. */
const LAG = 2.5;
/** Thinnest bar, as a fraction of the bar's full height: a dot, never nothing. */
const MIN_HEIGHT = 0.06;
/** How far one scroll animation runs (three hours of levels); a longer take starts another. */
const SPAN = 3 * 3600 * 30;

const bars: HTMLElement[] = [];
for (let i = 0; i < RING; i++) bars.push(strip.appendChild(document.createElement("i")));

let phase: Phase = "idle";
let pitch = 0;        // px from one bar to the next
let count = 0;        // next slot to fill; slots are numbered from the start of the take
let origin = 0;       // the slot at the wave's right edge when the scroll animation started
let scroller: Animation | null = null;
let lastHeight = 0;   // the previous level's bar height, for a little smoothing
let framePending = false;

// What this recording looked like from in here, reported to the log when it ends. A webview the
// OS has suspended draws nothing and hears no levels; the log then says so.
const take = { active: false, start: 0, frames: 0, levels: 0, peak: 0, visibility: "", tallest: 0 };
function startReport() {
  Object.assign(take, { active: true, start: performance.now(), frames: 0, levels: 0, peak: 0, visibility: document.visibilityState, tallest: 0 });
}
document.addEventListener("visibilitychange", () => { if (take.active) take.visibility += `>${document.visibilityState}`; });
function sendReport() {
  if (!take.active) return;
  take.active = false;
  const report = { ms: performance.now() - take.start, frames: take.frames, levels: take.levels, peak: take.peak, tallest: take.tallest, visibility: `${take.visibility}, ${document.visibilityState} at end` };
  invoke("pill_report", { report }).catch(() => {});
}

/** Measures the wave. It is hidden (zero wide) while the pill shows a message, so done lazily. */
function measure(): boolean {
  if (pitch > 0) return true;
  const width = wave.clientWidth;
  if (width <= 0) return false;
  pitch = width / BARS;
  // Bars are as wide as their slot minus a 2.5px gap, centred in it.
  strip.style.setProperty("--bar", `${Math.max(1, pitch - 2.5).toFixed(2)}px`);
  return true;
}

/** Puts one bar in slot `slot` of the scrolling strip, `height` a fraction (0..1) of full height. */
function place(slot: number, height: number) {
  const bar = bars[((slot % RING) + RING) % RING];
  bar.style.transform = `translate3d(${(slot * pitch).toFixed(2)}px,0,0) scaleY(${Math.max(MIN_HEIGHT, height).toFixed(3)})`;
}

/** The strip's offset that puts slot `edge` at the wave's right edge. */
function offset(edge: number): string {
  return `translate3d(${(-(edge - BARS + 1) * pitch).toFixed(2)}px,0,0)`;
}

/** The slot at the wave's right edge right now, fractional while moving. */
function edgeNow(): number {
  const t = Number(scroller?.currentTime ?? 0);
  return origin + t / LEVEL_MS;
}

/**
 * Starts the scroll from slot `edge`: one linear animation the compositor runs by itself, one
 * slot per level interval, so no script runs per frame.
 */
function startScroll(edge: number) {
  scroller?.cancel();
  origin = edge;
  scroller = strip.animate(
    [{ transform: offset(edge) }, { transform: offset(edge + SPAN) }],
    { duration: SPAN * LEVEL_MS, easing: "linear", fill: "forwards" },
  );
}

/** Adds the newest level (0..1 peak) as a bar entering on the right. */
function push(level: number) {
  // Boost quiet speech so the wave is lively; clamp to 1. A touch of the previous bar keeps the
  // outline from spiking bar to bar.
  const height = 0.7 * Math.min(1, Math.pow(level * 7, 0.65)) + 0.3 * lastHeight;
  lastHeight = height;
  const edge = edgeNow();
  if (edge >= origin + SPAN - BARS) startScroll(edge);
  // A slot is out of sight until the edge reaches it. A level that arrives after its slot came
  // into view (a late delivery) goes further right instead, so no bar ever pops up inside the
  // wave. Levels far ahead of the scroll (its clock was paused while the window was hidden) move
  // the scroll on to them.
  const hidden = Math.ceil(edge + 1);
  if (count < hidden) {
    // Slots already on screen get dots like the rest of the flat line; only the out-of-sight
    // ones get this level, which then scrolls in.
    const to = Math.ceil(edge + 1 + LAG);
    count = Math.max(count, to - RING);
    for (; count < to; count++) place(count, count < hidden ? 0 : height);
  } else {
    if (count > edge + 1 + LAG + 6) startScroll(count - 1 - LAG);
    place(count++, height);
  }
  if (take.active && take.levels % 30 === 0) {
    for (const b of bars) take.tallest = Math.max(take.tallest, b.getBoundingClientRect().height);
  }
  countFrame();
}

/**
 * Counts the redraws that show new levels, for the pill report: proof the page still draws. With
 * the scroll on the compositor these follow the level messages (about 15 a second), not the
 * display's frame rate.
 */
function countFrame() {
  if (framePending || !take.active) return;
  framePending = true;
  requestAnimationFrame(() => { framePending = false; if (take.active) take.frames++; });
}

/** Starts a take: flat dots across the wave, the strip moving so the next level enters right. */
function resetStrip() {
  if (!measure()) return;
  for (const bar of bars) bar.style.removeProperty("--x");
  count = 0;
  lastHeight = 0;
  // Dots in the visible slots and the ones the lag keeps hidden on the right.
  const prefill = BARS + Math.ceil(LAG) + 1;
  for (let i = 0; i < prefill; i++) place(count++, 0);
  startScroll(count - 1 - LAG);
}

/** Lines the bars up in fixed slots for the CSS animations of "processing" and "done". */
function parkBars() {
  scroller?.cancel();
  scroller = null;
  for (let i = 0; i < RING; i++) {
    const bar = bars[i];
    bar.style.transform = "";
    bar.style.setProperty("--x", i < BARS ? `${(i * pitch).toFixed(2)}px` : "-100px");
    bar.style.setProperty("--i", String(i));
  }
}

/** Errors always say something; "done" may carry a note, e.g. that the text went to the clipboard. */
function showMessage(p: Phase, message?: string) {
  const note = p === "done" && !!message;
  pill.className = note ? `${p} note` : p;
  msg.textContent = p === "error" ? (message ?? "Something went wrong") : note ? message! : "";
}

function enter(next: Phase, message?: string) {
  const was = phase;
  phase = next;
  showMessage(next, message);
  if (next === "recording") {
    resetStrip();
    // Replay the pop-in so the last take's faded-out frame never lingers.
    pill.style.animation = "none";
    void pill.offsetWidth;
    pill.style.animation = "";
  } else if (was === "recording" || next === "processing") {
    if (measure()) parkBars();
  }
}

// Levels arrive in small batches, oldest first.
listen<number[]>("level", (e) => {
  for (const level of e.payload) {
    if (take.active) { take.levels++; take.peak = Math.max(take.peak, level); }
    if (phase === "recording" && pitch > 0) push(level);
  }
});

// Sent when the hotkey is released, also for takes dropped as too short or silent.
listen("recording-ended", () => sendReport());

listen<StateEvent>("state", (e) => {
  const next = e.payload.phase;
  if (next !== "recording") sendReport();
  if (next === "recording") {
    sendReport();
    startReport();
  }
  enter(next, e.payload.message);
});

// Design/debug aid: drive the pill from the browser console or a preview without Tauri.
// window.__stfuDemo("recording" | "processing" | "done" | "error", message?)
let demoTimer: number | undefined;
(window as unknown as { __stfuDemo: (p: Phase, m?: string) => void }).__stfuDemo = (p, m) => {
  if (demoTimer) { clearInterval(demoTimer); demoTimer = undefined; }
  enter(p, m);
  if (p === "recording") {
    demoTimer = window.setInterval(() => push(Math.random() * 0.15), LEVEL_MS);
  }
};

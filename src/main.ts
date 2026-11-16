import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Phase = "recording" | "processing" | "done" | "error";
interface StateEvent { phase: Phase; message?: string }

const pill = document.getElementById("pill") as HTMLDivElement;
const msg = document.getElementById("msg") as HTMLDivElement;
const wave = document.getElementById("wave") as HTMLDivElement;

// The wave is plain elements, not a <canvas>. On macOS a canvas in this window could keep showing
// an old frame (the last take's orange "processing" wave) after the window was hidden and shown
// again, while the script went on drawing into it unseen. Elements go through the same path as
// the rest of the pill, which always repaints.
const BARS = 32;
const bars: HTMLElement[] = [];
for (let i = 0; i < BARS; i++) bars.push(wave.appendChild(document.createElement("i")));

const levels = new Float32Array(BARS);   // target heights 0..1, newest at the end
const shown = new Float32Array(BARS);    // eased heights actually drawn
let phase: Phase = "recording";
let phaseStart = performance.now();

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

/** Sets each bar's height as a fraction (0..1) of the wave's height; never thinner than a dot. */
function drawBars(heights: Float32Array) {
  for (let i = 0; i < BARS; i++) {
    bars[i].style.height = `max(2px, ${(heights[i] * 85).toFixed(1)}%)`;
  }
}

function frame(now: number) {
  const t = (now - phaseStart) / 1000;

  if (phase === "recording") {
    if (take.active) take.frames++;
    // Ease displayed bars towards live levels; scroll right-to-left as new samples arrive.
    for (let i = 0; i < BARS; i++) shown[i] += (levels[i] - shown[i]) * 0.45;
    drawBars(shown);
    // Now and then, measure what was actually laid out: proof in the log that the bars move.
    if (take.active && take.frames % 10 === 0) {
      for (const b of bars) take.tallest = Math.max(take.tallest, b.getBoundingClientRect().height);
    }
  } else if (phase === "processing") {
    // Travelling wave: two sines with different speeds, amplitude swells over the first 300 ms.
    const swell = Math.min(1, t / 0.3);
    for (let i = 0; i < BARS; i++) {
      const x = i / (BARS - 1);
      const v = 0.5 + 0.5 * Math.sin(x * 9 - t * 7) * Math.sin(x * 3.5 + t * 2.2);
      const target = (0.15 + v * 0.75) * swell;
      shown[i] += (target - shown[i]) * 0.35;
    }
    drawBars(shown);
  } else if (phase === "done") {
    // Settle to a flat line; the ripple out from the centre is a CSS animation (index.html).
    for (let i = 0; i < BARS; i++) shown[i] += (0.06 - shown[i]) * 0.25;
    drawBars(shown);
  }
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);

/** Errors always say something; "done" may carry a note, e.g. that the text went to the clipboard. */
function showMessage(p: Phase, message?: string) {
  const note = p === "done" && !!message;
  pill.className = note ? `${p} note` : p;
  msg.textContent = p === "error" ? (message ?? "Something went wrong") : note ? message! : "";
}

listen<number>("level", (e) => {
  if (take.active) { take.levels++; take.peak = Math.max(take.peak, e.payload); }
  levels.copyWithin(0, 1);
  // Boost quiet speech so the wave is lively; clamp to 1.
  levels[BARS - 1] = Math.min(1, Math.pow(e.payload * 7, 0.65));
});

// Sent when the hotkey is released, also for takes dropped as too short or silent.
listen("recording-ended", () => sendReport());

listen<StateEvent>("state", (e) => {
  const next = e.payload.phase;
  if (next !== phase) phaseStart = performance.now();
  if (next !== "recording") sendReport();
  phase = next;
  showMessage(phase, e.payload.message);
  if (phase === "recording") {
    sendReport();
    startReport();
    levels.fill(0); shown.fill(0);
    drawBars(shown);
    // Replay the pop-in so the last take's faded-out frame never lingers.
    pill.style.animation = "none";
    void pill.offsetWidth;
    pill.style.animation = "";
  }
});

// Design/debug aid: drive the pill from the browser console or a preview without Tauri.
// window.__stfuDemo("recording" | "processing" | "done" | "error", message?)
let demoTimer: number | undefined;
(window as unknown as { __stfuDemo: (p: Phase, m?: string) => void }).__stfuDemo = (p, m) => {
  if (demoTimer) { clearInterval(demoTimer); demoTimer = undefined; }
  if (p !== phase) phaseStart = performance.now();
  phase = p;
  showMessage(p, m);
  if (p === "recording") {
    levels.fill(0); shown.fill(0);
    demoTimer = window.setInterval(() => {
      levels.copyWithin(0, 1);
      levels[BARS - 1] = Math.min(1, Math.pow(Math.random() * 0.15 * 7, 0.65));
    }, 33);
  }
};

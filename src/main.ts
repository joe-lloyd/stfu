import { listen } from "@tauri-apps/api/event";

type Phase = "recording" | "processing" | "done" | "error";
interface StateEvent { phase: Phase; message?: string }

const pill = document.getElementById("pill") as HTMLDivElement;
const msg = document.getElementById("msg") as HTMLDivElement;
const canvas = document.getElementById("wave") as HTMLCanvasElement;
const ctx = canvas.getContext("2d")!;

const BARS = 32;
const levels = new Float32Array(BARS);   // target heights 0..1, newest at the end
const shown = new Float32Array(BARS);    // eased heights actually drawn
let phase: Phase = "recording";
let phaseStart = performance.now();

const COLORS: Record<Phase, string> = {
  recording: "#8fd6ff",
  processing: "#f5c542",
  done: "#3ddc84",
  error: "#ff4d4f",
};

function resize() {
  const dpr = window.devicePixelRatio || 1;
  canvas.width = canvas.clientWidth * dpr;
  canvas.height = canvas.clientHeight * dpr;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
}
window.addEventListener("resize", resize);
resize();

function drawBars(heights: Float32Array, color: string, alpha = 1) {
  const w = canvas.clientWidth, h = canvas.clientHeight;
  const gap = 2.5;
  const bw = (w - gap * (BARS - 1)) / BARS;
  ctx.globalAlpha = alpha;
  ctx.fillStyle = color;
  for (let i = 0; i < BARS; i++) {
    const bh = Math.max(2, heights[i] * h * 0.85);
    const x = i * (bw + gap);
    ctx.beginPath();
    ctx.roundRect(x, (h - bh) / 2, bw, bh, bw / 2);
    ctx.fill();
  }
  ctx.globalAlpha = 1;
}

function frame(now: number) {
  const w = canvas.clientWidth, h = canvas.clientHeight;
  ctx.clearRect(0, 0, w, h);
  const t = (now - phaseStart) / 1000;

  if (phase === "recording") {
    // Ease displayed bars towards live levels; scroll right-to-left as new samples arrive.
    for (let i = 0; i < BARS; i++) shown[i] += (levels[i] - shown[i]) * 0.45;
    drawBars(shown, COLORS.recording);
  } else if (phase === "processing") {
    // Travelling wave: two sines with different speeds, amplitude swells over the first 300 ms.
    const swell = Math.min(1, t / 0.3);
    for (let i = 0; i < BARS; i++) {
      const x = i / (BARS - 1);
      const v = 0.5 + 0.5 * Math.sin(x * 9 - t * 7) * Math.sin(x * 3.5 + t * 2.2);
      const target = (0.15 + v * 0.75) * swell;
      shown[i] += (target - shown[i]) * 0.35;
    }
    drawBars(shown, COLORS.processing);
  } else if (phase === "done") {
    // Settle to a flat line while a ripple runs out from the centre, then a soft glow.
    for (let i = 0; i < BARS; i++) shown[i] += (0.06 - shown[i]) * 0.25;
    drawBars(shown, COLORS.done);
    const r = Math.min(1, t / 0.45);
    if (r < 1) {
      const cx = w / 2, cy = h / 2;
      ctx.strokeStyle = COLORS.done;
      ctx.globalAlpha = (1 - r) * 0.9;
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.ellipse(cx, cy, 6 + r * (w / 2), Math.min(cy - 2, 4 + r * 10), 0, 0, Math.PI * 2);
      ctx.stroke();
      ctx.globalAlpha = 1;
    }
  }
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);

listen<number>("level", (e) => {
  levels.copyWithin(0, 1);
  // Boost quiet speech so the wave is lively; clamp to 1.
  levels[BARS - 1] = Math.min(1, Math.pow(e.payload * 7, 0.65));
});

listen<StateEvent>("state", (e) => {
  const next = e.payload.phase;
  if (next !== phase) phaseStart = performance.now();
  phase = next;
  pill.className = phase;
  msg.textContent = phase === "error" ? (e.payload.message ?? "Something went wrong") : "";
  if (phase === "recording") { levels.fill(0); shown.fill(0); }
});

// Design/debug aid: drive the pill from the browser console or a preview without Tauri.
// window.__stfuDemo("recording" | "processing" | "done" | "error")
let demoTimer: number | undefined;
(window as unknown as { __stfuDemo: (p: Phase, m?: string) => void }).__stfuDemo = (p, m) => {
  if (demoTimer) { clearInterval(demoTimer); demoTimer = undefined; }
  if (p !== phase) phaseStart = performance.now();
  phase = p;
  pill.className = p;
  msg.textContent = p === "error" ? (m ?? "Something went wrong") : "";
  if (p === "recording") {
    levels.fill(0); shown.fill(0);
    demoTimer = window.setInterval(() => {
      levels.copyWithin(0, 1);
      levels[BARS - 1] = Math.min(1, Math.pow(Math.random() * 0.15 * 7, 0.65));
    }, 33);
  }
};

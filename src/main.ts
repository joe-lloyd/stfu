import { listen } from "@tauri-apps/api/event";

type Phase = "recording" | "processing" | "done" | "error";
interface StateEvent { phase: Phase; message?: string }

const pill = document.getElementById("pill") as HTMLDivElement;
const label = document.getElementById("label") as HTMLDivElement;
const canvas = document.getElementById("wave") as HTMLCanvasElement;
const ctx = canvas.getContext("2d")!;

const BARS = 40;
const levels = new Float32Array(BARS);
let phase: Phase = "recording";

function resize() {
  const dpr = window.devicePixelRatio || 1;
  canvas.width = canvas.clientWidth * dpr;
  canvas.height = canvas.clientHeight * dpr;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
}
window.addEventListener("resize", resize);
resize();

function draw() {
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  ctx.clearRect(0, 0, w, h);
  const gap = 2;
  const bw = (w - gap * (BARS - 1)) / BARS;
  for (let i = 0; i < BARS; i++) {
    const lvl = phase === "recording" ? levels[i] : levels[i] * 0.3;
    const bh = Math.max(2, lvl * h);
    const x = i * (bw + gap);
    const y = (h - bh) / 2;
    ctx.fillStyle = phase === "recording" ? "#7dd3fc" : "#666";
    ctx.beginPath();
    ctx.roundRect(x, y, bw, bh, bw / 2);
    ctx.fill();
  }
  requestAnimationFrame(draw);
}
requestAnimationFrame(draw);

listen<number>("level", (e) => {
  levels.copyWithin(0, 1);
  // Boost quiet speech so the wave is visible; clamp to 1.
  levels[BARS - 1] = Math.min(1, Math.pow(e.payload * 6, 0.7));
});

listen<StateEvent>("state", (e) => {
  phase = e.payload.phase;
  pill.className = phase;
  label.textContent =
    e.payload.message ??
    ({ recording: "Listening", processing: "Thinking…", done: "Done", error: "Error" } as const)[phase];
  if (phase === "recording") levels.fill(0);
});

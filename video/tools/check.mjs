// Checks the render: a labelled contact sheet with a frame from every chapter plus one every 20 s,
// and the loudness of each dictation next to the narration line before it.
//
//   node video/tools/check.mjs   # after page.ts → video/build/sheets/render.png
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const VIDEO = join(dirname(fileURLToPath(import.meta.url)), "..");
const BUILD = join(VIDEO, "build");
const TOOLS = join(homedir(), ".neon-video", "tools");
const RENDER = join(BUILD, "renders", "stfu.mp4");
const FPS = 30;
const duration = Number(execFileSync(join(TOOLS, "ffprobe"), ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", RENDER]).toString());
const clock = (s) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;

const points = [];
for (const line of readFileSync(join(BUILD, "publish", "chapters.txt"), "utf8").trim().split("\n")) {
  const [, m, s, title] = /^(\d+):(\d+) (.*)$/.exec(line);
  const at = Number(m) * 60 + Number(s) + 2;
  points.push([at, `${clock(at)} ${title}`]);
}
for (let t = 20; t < duration; t += 20) points.push([t, clock(t)]);
points.push([duration - 0.5, `${clock(duration)} end`]);
points.sort((a, b) => a[0] - b[0]);

const tmp = join(BUILD, "sheets", "render");
rmSync(tmp, { recursive: true, force: true });
mkdirSync(tmp, { recursive: true });
points.forEach(([t, label], i) => {
  execFileSync(join(TOOLS, "ffmpeg"), ["-loglevel", "error", "-y", "-ss", String(t), "-i", RENDER, "-frames:v", "1",
    "-vf", `scale=480:-1,drawtext=fontfile=/System/Library/Fonts/Helvetica.ttc:text='${label.replace(/[':]/g, (c) => `\\${c}`)}':x=6:y=6:fontsize=18:fontcolor=white:box=1:boxcolor=black@0.7`, join(tmp, `${String(i).padStart(3, "0")}.png`)]);
});
const out = join(BUILD, "sheets", "render.png");
execFileSync(join(TOOLS, "ffmpeg"), ["-loglevel", "error", "-y", "-pattern_type", "glob", "-i", join(tmp, "*.png"), "-vf", `tile=5x${Math.ceil(points.length / 5)}`, out]);
rmSync(tmp, { recursive: true, force: true });

/** Integrated loudness of the render between two times, as ffmpeg's ebur128 summary reports it. */
function lufs(from, length) {
  const log = execFileSync("sh", ["-c", `"${join(TOOLS, "ffmpeg")}" -hide_banner -nostats -ss ${from} -t ${length} -i "${RENDER}" -af ebur128 -f null - 2>&1`]).toString();
  return /I:\s+(-?[\d.]+) LUFS\s*\n\s*Threshold/.exec(log)?.[1] ?? "?";
}

const plan = JSON.parse(readFileSync(join(BUILD, "neon", "stfu-plan.json"), "utf8"));
const script = JSON.parse(readFileSync(join(BUILD, "neon", "stfu-script.json"), "utf8"));
const voLines = script.sections.flatMap((s) => s.lines).map((l) => ({ at: l.at / FPS, duration: l.duration / FPS }));
for (const op of plan.ops) {
  const id = /^Dictation (\w+)$/.exec(op.body.name ?? "")?.[1];
  if (!id) continue;
  const at = op.body.at / FPS;
  const clip = JSON.parse(readFileSync(join(BUILD, "dictation", `${id}.json`), "utf8"));
  const before = voLines.filter((l) => l.at < at).at(-1);
  const after = voLines.find((l) => l.at > at);
  const overlap = (before && before.at + before.duration > at) || (after && after.at < at + clip.duration);
  console.log(`${id} at ${clock(at)}: dictation ${lufs(at, clip.duration)} LUFS · narration before ${before ? lufs(before.at, before.duration) : "-"} LUFS · overlap ${overlap ? "YES" : "no"}`);
}
console.log(out, `${points.length} frames, ${clock(duration)} (${duration.toFixed(1)} s)`);

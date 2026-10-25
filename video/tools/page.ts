// Writes everything the release needs from the rendered tour, and the watch page.
//
//   node video/tools/page.ts   # after edit.ts --render, with the Neon project open
//
// video/build/publish/: description.md, chapters.txt, captions.vtt, captions.srt, title.txt and
// tags.txt. ~/htmlify/videos/stfu-tour/: the watch page with the video, chapters and captions.
// Line times come from the open Neon project, so changes made in the editor are reflected.
import { execFileSync } from "node:child_process";
import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { VoLine } from "./director.ts";
import { neon } from "./neon.ts";

const VIDEO = join(dirname(fileURLToPath(import.meta.url)), "..");
const BUILD = join(VIDEO, "build");
const PUBLISH = join(BUILD, "publish");
const WATCH = join(homedir(), "htmlify", "videos", "stfu-tour");
const RENDER = join(BUILD, "renders", "stfu.mp4");
const TOOLS = join(homedir(), ".neon-video", "tools");
const FPS = 30;
const REPO = "https://github.com/joe-lloyd/stfu";

type Script = { youtube: { title: string }; dictations: Record<string, { say: string }>; sections: { title: string; lines: { say: string }[] }[] };
const script: Script = JSON.parse(await readFile(join(VIDEO, "script.json"), "utf8"));
type ScriptState = { sections: { title: string; lines: { say: string; placed?: { startFrame: number } }[] }[] };
const state = neon<ScriptState>("api", "GET", "script");
const vo = new Map<string, VoLine>(JSON.parse(await readFile(join(BUILD, "vo", "manifest.json"), "utf8")).lines.map((l: VoLine) => [l.id, l]));
const duration = Number(execFileSync(join(TOOLS, "ffprobe"), ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", RENDER]).toString());

const clock = (seconds: number) => `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;
const stamp = (seconds: number, sep: "." | ",") => {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${s.toFixed(3).padStart(6, "0").replace(".", sep)}`;
};

// Chapters: each section from its first line, the first from 0:00. YouTube wants each 10 s or more.
const chapters = state.sections.map((section, s) => ({ title: section.title, at: s === 0 ? 0 : Math.floor((section.lines[0]?.placed?.startFrame ?? 0) / FPS) }));
chapters.forEach((c, i) => {
  const end = chapters[i + 1]?.at ?? duration;
  if (end - c.at < 10) throw new Error(`Chapter "${c.title}" is ${(end - c.at).toFixed(1)} s; YouTube needs 10 s`);
});

// Captions: the narration, split at sentence ends or every eight words and timed from the voice's
// word timings, plus the dictations, marked as such, from where the edit plays them.
type Cue = { from: number; to: number; text: string };
const cues: Cue[] = [];
function caption(start: number, written: string, words: { start: number; end: number }[], prefix = "") {
  const shown = written.split(/\s+/).filter(Boolean);
  if (!words.length) return;
  const perWord = words.length / shown.length;
  let chunk: { text: string[]; from: number; to: number } | undefined;
  const flush = () => {
    if (chunk) cues.push({ from: start + chunk.from, to: start + chunk.to, text: `${prefix}${chunk.text.join(" ")}` });
    chunk = undefined;
  };
  shown.forEach((text, i) => {
    const w = words[Math.min(words.length - 1, Math.floor(i * perWord))]!;
    chunk ??= { text: [], from: w.start, to: w.end };
    chunk.text.push(text);
    chunk.to = w.end;
    if (/[.?!:]$/.test(text) || chunk.text.length >= 8) flush();
  });
  flush();
}
for (const [s, section] of state.sections.entries()) {
  for (const [l, line] of section.lines.entries()) {
    if (!line.placed) continue;
    // Captions show the script's spelling ("STFU", "Fn"), not the spoken respelling.
    caption(line.placed.startFrame / FPS, script.sections[s]!.lines[l]!.say, (vo.get(`${s + 1}.${l + 1}`)?.words ?? []).filter((w) => /\w/.test(w.word)));
  }
}
const plan: { ops: { body: { kind?: string; name?: string; at?: number } }[] } = JSON.parse(await readFile(join(BUILD, "neon", "stfu-plan.json"), "utf8"));
for (const op of plan.ops) {
  const id = /^Dictation (\w+)$/.exec(op.body.name ?? "")?.[1];
  if (!id || op.body.kind !== "audio") continue;
  const clip: { duration: number } = JSON.parse(await readFile(join(BUILD, "dictation", `${id}.json`), "utf8"));
  // voice.mjs keeps only the dictation's loudness, so its words are spread evenly over the clip.
  const said = script.dictations[id]!.say.split(/\s+/);
  const even = said.map((_, i) => ({ start: (i / said.length) * clip.duration, end: ((i + 1) / said.length) * clip.duration }));
  caption(op.body.at! / FPS, script.dictations[id]!.say, even, "(dictating) ");
}
cues.sort((a, b) => a.from - b.from);
const vtt = ["WEBVTT", "", ...cues.flatMap((c) => [`${stamp(c.from, ".")} --> ${stamp(c.to, ".")}`, c.text, ""])].join("\n");
const srt = cues.flatMap((c, i) => [String(i + 1), `${stamp(c.from, ",")} --> ${stamp(c.to, ",")}`, c.text, ""]).join("\n");

const title = script.youtube.title;
const tags = ["STFU", "dictation", "speech to text", "voice typing", "Whisper", "Groq", "Ollama", "OpenCode", "Tauri", "Rust", "macOS", "Windows", "productivity", "open source"];
const description = [
  "STFU is a small desktop app: hold a key, talk, and let go. Your speech is transcribed, cleaned up by a language model and pasted wherever your cursor is. Fillers go, corrections are applied, punctuation is fixed and spoken lists become bullets.",
  "",
  "This tour covers a dictation from start to finish, how the pipeline works, the cleanup rules, the providers (Groq by default, OpenCode Zen, OpenAI and a local Ollama), what happens when there is nowhere to paste, the local history with its limits and lossless compression, the Windows hotkey, auto-update, launch at login and the one-line installers.",
  "",
  "Chapters",
  ...chapters.map((c) => `${clock(c.at)} ${c.title}`),
  "",
  `Code: ${REPO}`,
  `Releases and installers: ${REPO}/releases`,
  "",
  "macOS or Linux:",
  "curl -fsSL https://raw.githubusercontent.com/joe-lloyd/stfu/main/scripts/install.sh | sh",
  "Windows (PowerShell):",
  "irm https://raw.githubusercontent.com/joe-lloyd/stfu/main/scripts/install.ps1 | iex",
  "",
  "How this was made: the pill and the Settings window on screen are STFU's real web views, run from the repository in Chromium with the Tauri calls answered by a stand-in for the Rust core. The desktops, the apps being typed into and the dictations are staged: the person dictating is a second synthetic voice, and the cleaned text is written to match what the cleanup rules produce.",
  "",
  "The narration is a synthetic voice (Kokoro).",
].join("\n");

await mkdir(PUBLISH, { recursive: true });
await writeFile(join(PUBLISH, "description.md"), `${description}\n`);
await writeFile(join(PUBLISH, "chapters.txt"), `${chapters.map((c) => `${clock(c.at)} ${c.title}`).join("\n")}\n`);
await writeFile(join(PUBLISH, "captions.vtt"), vtt);
await writeFile(join(PUBLISH, "captions.srt"), srt);
await writeFile(join(PUBLISH, "title.txt"), `${title}\n`);
await writeFile(join(PUBLISH, "tags.txt"), `${tags.join(", ")}\n`);

// The watch page, with its video, poster and captions next to it.
await mkdir(WATCH, { recursive: true });
await copyFile(RENDER, join(WATCH, "stfu-tour.mp4"));
await writeFile(join(WATCH, "captions.vtt"), vtt);
await copyFile(join(BUILD, "thumbs", "stfu.png"), join(WATCH, "poster.png"));
const escape = (text: string) => text.replace(/&/g, "&amp;").replace(/</g, "&lt;");
const html = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>STFU: a tour</title>
<style>
  :root { color-scheme: dark; }
  body { margin: 0; background: #16161a; color: #e8e8ec; font: 16px/1.6 -apple-system, BlinkMacSystemFont, "Segoe UI", Inter, sans-serif; }
  main { max-width: 1120px; margin: 0 auto; padding: 40px 24px 80px; }
  .kicker { color: #7dd3fc; font-weight: 700; font-size: 12px; letter-spacing: .12em; text-transform: uppercase; }
  h1 { font-size: 34px; line-height: 1.15; margin: 8px 0 6px; }
  .lede { color: #9a9aa5; margin: 0 0 24px; }
  video { width: 100%; border-radius: 12px; background: #000; box-shadow: 0 20px 60px rgba(0,0,0,.5); }
  .grid { display: grid; grid-template-columns: 1fr 320px; gap: 28px; margin-top: 28px; }
  h2 { font-size: 18px; margin: 0 0 10px; }
  ol { list-style: none; margin: 0; padding: 0; }
  ol button { display: flex; gap: 12px; width: 100%; text-align: left; background: none; border: 0; color: inherit; font: inherit; padding: 7px 10px; border-radius: 8px; cursor: pointer; }
  ol button:hover { background: #1f1f25; }
  ol time { color: #7dd3fc; font-variant-numeric: tabular-nums; min-width: 40px; }
  a { color: #7dd3fc; }
  ul { padding-left: 20px; margin: 0; }
  li { margin: 4px 0; }
  code { background: #1f1f25; padding: 1px 5px; border-radius: 4px; font-size: 13px; }
  @media (max-width: 860px) { .grid { grid-template-columns: 1fr; } }
</style>
</head>
<body>
<main>
  <div class="kicker">STFU · tour</div>
  <h1>${escape(title)}</h1>
  <p class="lede">Hold a key, talk, release. ${clock(duration)} minutes.</p>
  <video id="v" controls preload="metadata" poster="poster.png">
    <source src="stfu-tour.mp4" type="video/mp4">
    <track kind="captions" src="captions.vtt" srclang="en" label="English" default>
  </video>
  <div class="grid">
    <section>
      <h2>How this video was made</h2>
      <ul>
        <li>The pill and the Settings window are STFU's real web views from <a href="${REPO}">the repository</a>, run in Chromium. A stand-in answers the calls the Rust core would, with the same messages.</li>
        <li>The desktops, the apps being typed into and the dictations are staged. The person dictating is a second synthetic voice, and the waveform follows that voice's loudness.</li>
        <li>The narration is a synthetic voice (Kokoro, <code>af_heart</code>).</li>
        <li>The edit was built and rendered in Neon Video Studio. Every voice-over clip is linked to its script line, so any line can be re-recorded.</li>
        <li>The production kit is in the repository under <code>video/</code> on the <code>video/tour</code> branch.</li>
      </ul>
    </section>
    <section>
      <h2>Chapters</h2>
      <ol>
        ${chapters.map((c) => `<li><button data-t="${c.at}"><time>${clock(c.at)}</time><span>${escape(c.title)}</span></button></li>`).join("\n        ")}
      </ol>
    </section>
  </div>
</main>
<script>
  const video = document.getElementById("v");
  for (const button of document.querySelectorAll("[data-t]")) button.addEventListener("click", () => { video.currentTime = Number(button.dataset.t); video.play(); });
</script>
</body>
</html>
`;
await writeFile(join(WATCH, "index.html"), html);
process.stdout.write(`${PUBLISH}\n${join(WATCH, "index.html")}\n${chapters.length} chapters, ${cues.length} captions, ${clock(duration)}\n`);

// Narrates every line of script.json, and voices the dictations, with the local Kokoro TTS server.
//
//   node video/tools/voice.mjs
//
// Narration: video/build/vo/<section>.<line>.mp3 plus manifest.json with each line's duration and
// word timings. Dictations: video/build/dictation/<id>.mp3 plus <id>.json with the loudness of
// every 1/30 s, which feeds the pill's waveform. Unchanged lines are not regenerated.
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const VIDEO = join(dirname(fileURLToPath(import.meta.url)), "..");
const KOKORO = process.env.KOKORO_URL ?? "http://127.0.0.1:8882";
// The voice of the Git Gud, htmlify and Open Chinese tours, so they sound like one channel.
const VOICE = process.env.VOICE ?? "af_heart";
const TOOLS = join(homedir(), ".neon-video", "tools");
const FPS = 30;

/** Spoken forms for words the model reads wrong. Applied to the audio only, never to captions. */
const PRONOUNCE = [
  [/\bSTFU\b/g, "S T F U"],
  [/\bFn\b/g, "F N"],
  [/\binto sh\b/g, "into S H"],
  [/\bLLM\b/g, "L L M"],
  [/\bFLAC\b/g, "flack"],
  [/\bGroq\b/g, "Grock"],
  [/\bQwen\b/g, "Kwen"],
  [/\bwhisper\.cpp\b/g, "whisper C P P"],
  [/\bOpenAI\b/g, "Open A I"],
  [/\bOllama\b/g, "Oh-llama"],
  [/\bTauri\b/g, "Tor-ee"],
  [/\bGitHub\b/g, "Git Hub"],
  [/\bmacOS\b/g, "mac O S"],
  [/\bjoe-lloyd\b/g, "joe lloyd"],
  [/\bv3\b/g, "v three"],
];
const spoken = (text) => PRONOUNCE.reduce((t, [pattern, replacement]) => t.replace(pattern, replacement), text);

const duration = (file) => Number(execFileSync(join(TOOLS, "ffprobe"), ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", file]).toString().trim());

async function speak(voice, input, file) {
  const response = await fetch(`${KOKORO}/dev/captioned_speech`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ model: "kokoro", voice, input, response_format: "mp3", stream: false }),
  });
  if (!response.ok) throw new Error(`Kokoro failed: HTTP ${response.status} ${await response.text()}`);
  const body = await response.json();
  await writeFile(file, Buffer.from(body.audio, "base64"));
  return (body.timestamps ?? []).map((w) => ({ word: w.word, start: w.start_time, end: w.end_time }));
}

/** RMS loudness of each 1/30 s of `file`, the same measure the app sends the pill. */
function levels(file) {
  const pcm = execFileSync(join(TOOLS, "ffmpeg"), ["-v", "error", "-i", file, "-ac", "1", "-ar", "16000", "-f", "s16le", "-"], { maxBuffer: 1 << 28 });
  const samples = new Int16Array(pcm.buffer, pcm.byteOffset, pcm.length / 2);
  const per = 16000 / FPS;
  const out = [];
  for (let k = 0; k * per < samples.length; k++) {
    let sum = 0;
    const i = Math.floor(k * per);
    const end = Math.min(samples.length, Math.floor((k + 1) * per));
    for (let j = i; j < end; j++) sum += (samples[j] / 32768) ** 2;
    out.push(Number(Math.sqrt(sum / Math.max(1, end - i)).toFixed(4)));
  }
  return out;
}

const script = JSON.parse(await readFile(join(VIDEO, "script.json"), "utf8"));

// Narration.
const out = join(VIDEO, "build", "vo");
await mkdir(out, { recursive: true });
const manifestPath = join(out, "manifest.json");
const previous = existsSync(manifestPath) ? JSON.parse(await readFile(manifestPath, "utf8")) : { lines: [] };
const lines = [];
for (const [s, section] of script.sections.entries()) {
  for (const [l, line] of section.lines.entries()) {
    const id = `${s + 1}.${l + 1}`;
    const file = join(out, `${id}.mp3`);
    const cached = previous.lines.find((p) => p.say === line.say && p.voice === VOICE && existsSync(join(out, p.file)));
    if (cached) {
      // Lines move when the script changes; reuse the audio under the new id.
      if (cached.id !== id) await writeFile(file, await readFile(join(out, cached.file)));
      lines.push({ ...cached, id, file: `${id}.mp3` });
      continue;
    }
    const words = await speak(VOICE, spoken(line.say), file);
    const entry = { id, say: line.say, voice: VOICE, file: `${id}.mp3`, duration: duration(file), words };
    lines.push(entry);
    process.stdout.write(`${id}  ${entry.duration.toFixed(2)}s  ${line.say.slice(0, 60)}\n`);
  }
}
const total = lines.reduce((sum, l) => sum + l.duration, 0);
await writeFile(manifestPath, `${JSON.stringify({ voice: VOICE, total, lines }, null, 2)}\n`);
process.stdout.write(`narration: ${lines.length} lines, ${(total / 60).toFixed(1)} minutes\n`);

// Dictations, in a second voice: the person talking to STFU on screen.
const dictDir = join(VIDEO, "build", "dictation");
await mkdir(dictDir, { recursive: true });
for (const [id, d] of Object.entries(script.dictations)) {
  const file = join(dictDir, `${id}.mp3`);
  const meta = join(dictDir, `${id}.json`);
  const old = existsSync(meta) ? JSON.parse(await readFile(meta, "utf8")) : undefined;
  if (old && old.say === d.say && old.voice === d.voice && existsSync(file)) continue;
  await speak(d.voice, d.say, file);
  const entry = { id, say: d.say, voice: d.voice, duration: duration(file), levels: levels(file) };
  await writeFile(meta, JSON.stringify(entry));
  process.stdout.write(`dictation ${id}  ${entry.duration.toFixed(2)}s  ${d.say.slice(0, 60)}\n`);
}

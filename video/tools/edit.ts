// Builds the STFU tour's edit in Neon Video Studio from the recorded takes and the narration.
//
//   node video/tools/neon.ts serve --detach    # a private Neon instance for this video
//   node video/tools/edit.ts                   # builds ~/Videos/tours/stfu.neon
//   node video/tools/edit.ts --render          # and renders video/build/renders/stfu.mp4
//
// Takes go on V1, the narration on A1 with every clip linked to its script line, the dictation
// voice on A2, and titles, diagrams, chapter labels, key combos and callouts on FX1 and FX2.
import { execFileSync } from "node:child_process";
import { existsSync, statSync } from "node:fs";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { LINE_GAP, type TakeResult, type VoLine } from "./director.ts";
import { neon, PROJECT } from "./neon.ts";

const VIDEO = join(dirname(fileURLToPath(import.meta.url)), "..");
const BUILD = join(VIDEO, "build");
const FPS = 30;
const ACCENT = "#7dd3fc";
const FFMPEG = join(homedir(), ".neon-video", "tools", "ffmpeg");

type Fx = { name: string; props: Record<string, unknown> };
type Line = { say: string; show?: string; take?: string; fx?: Fx };
type Script = { title: string; sections: { title: string; intent?: string; take?: string; lines: Line[] }[]; dictations: Record<string, { say: string }> };

const script: Script = JSON.parse(await readFile(join(VIDEO, "script.json"), "utf8"));
const voDir = join(BUILD, "vo");
const vo: Map<string, VoLine> = new Map(JSON.parse(await readFile(join(voDir, "manifest.json"), "utf8")).lines.map((l: VoLine) => [l.id, l]));
const takesDir = join(BUILD, "takes");

/** What is on screen for each line: a recorded take, or an STFU FX component. */
type Visual = { kind: "take"; take: string } | { kind: "fx"; fx: Fx };
const lines = script.sections.flatMap((section, s) => section.lines.map((line, l) => ({ id: `${s + 1}.${l + 1}`, section: s + 1, line, sectionTake: section.take })));
const visualFor = (entry: (typeof lines)[number]): Visual => {
  if (entry.line.fx) return { kind: "fx", fx: entry.line.fx };
  const take = entry.line.take ?? entry.sectionTake;
  if (!take) throw new Error(`Line ${entry.id} has neither fx nor take`);
  return { kind: "take", take };
};
const sameVisual = (a: Visual, b: Visual) => JSON.stringify(a) === JSON.stringify(b);

const takes = new Map<string, TakeResult>();
for (const entry of lines) {
  const visual = visualFor(entry);
  if (visual.kind === "take" && !takes.has(visual.take)) takes.set(visual.take, JSON.parse(await readFile(join(takesDir, `${visual.take}.json`), "utf8")));
}

// Lay the lines out end to end. A run of lines from one take keeps the take's own timing.
type Placed = { id: string; at: number; duration: number; section: number };
type Segment = { visual: Visual; at: number; length: number; trim: number; lines: Placed[] };
const segments: Segment[] = [];
let cursor = 0;
for (let i = 0; i < lines.length; ) {
  const visual = visualFor(lines[i]!);
  let j = i;
  while (j + 1 < lines.length && sameVisual(visualFor(lines[j + 1]!), visual)) j++;
  const run = lines.slice(i, j + 1);
  if (visual.kind === "take") {
    const take = takes.get(visual.take)!;
    const startOf = (lineId: string) => {
      const found = take.lines.find((l) => l.id === lineId);
      if (!found) throw new Error(`Take ${visual.take} has no line ${lineId}`);
      return found.start;
    };
    // A line ends where the take's next line starts, so the action after the words stays in.
    const endOf = (lineId: string) => take.lines.filter((l) => l.start > startOf(lineId)).sort((a, b) => a.start - b.start)[0]?.start ?? take.duration;
    const trim = startOf(run[0]!.id);
    const length = endOf(run.at(-1)!.id) - trim;
    segments.push({ visual, at: cursor, length, trim, lines: run.map((r) => ({ id: r.id, section: r.section, at: cursor + startOf(r.id) - trim, duration: vo.get(r.id)!.duration })) });
    cursor += length;
  } else {
    const placed: Placed[] = [];
    let t = cursor + 0.4;
    for (const r of run) {
      placed.push({ id: r.id, section: r.section, at: t, duration: vo.get(r.id)!.duration });
      t += vo.get(r.id)!.duration + LINE_GAP;
    }
    // The end card holds a little longer, so the last chapter is long enough for YouTube.
    const tail = j === lines.length - 1 ? 3 : 0.3;
    segments.push({ visual, at: cursor, length: t - cursor + tail, trim: 0, lines: placed });
    cursor = t + tail;
  }
  i = j + 1;
}
const total = cursor;
process.stdout.write(`${segments.length} segments, ${(total / 60).toFixed(2)} minutes\n`);
const frames = (seconds: number) => Math.round(seconds * FPS);

// Narration at -16 LUFS. The dictation voice sits a touch under it, like a second speaker in the room.
async function normalise(source: string, out: string, lufs: number) {
  if (existsSync(out) && statSync(out).mtimeMs >= statSync(source).mtimeMs) return;
  await mkdir(dirname(out), { recursive: true });
  execFileSync(FFMPEG, ["-y", "-loglevel", "error", "-i", source, "-af", `loudnorm=I=${lufs}:TP=-1.5:LRA=7`, "-ar", "48000", "-c:a", "aac", "-b:a", "192k", out]);
}
for (const lineId of vo.keys()) await normalise(join(voDir, `${lineId}.mp3`), join(BUILD, "vo-norm", `${lineId}.m4a`), -16);
for (const id of Object.keys(script.dictations)) await normalise(join(BUILD, "dictation", `${id}.mp3`), join(BUILD, "dictation-norm", `${id}.m4a`), -17);

// A fresh project each run.
await mkdir(dirname(PROJECT), { recursive: true });
neon("project", "new", "--name", "STFU: a tour", "--fps", String(FPS), "--width", "1920", "--height", "1080");
if (existsSync(PROJECT)) await rm(PROJECT, { recursive: true, force: true });
neon("project", "save", PROJECT);
neon("packs", "install", join(VIDEO, "fx", "stfu-fx"));
type Track = { id: string; name: string; kind: string };
const listTracks = () => neon<{ tracks: Track[] }>("timeline", "show").tracks;
neon("tracks", "add", "--kind", "overlay", "--name", "FX2");
neon("tracks", "add", "--kind", "audio", "--name", "A2");
const track = (name: string) => {
  const found = listTracks().find((t) => t.name === name);
  if (!found) throw new Error(`No track ${name}: ${listTracks().map((t) => t.name).join(", ")}`);
  return found.id;
};
const V1 = track("V1");
const A1 = track("A1");
const A2 = track("A2");
const FX1 = track("FX1");
const FX2 = track("FX2");

const importAsset = (path: string) => neon<{ asset: { id: string } }>("api", "POST", "assets/import", JSON.stringify({ path })).asset.id;
const takeAssets = new Map([...takes.keys()].map((name) => [name, importAsset(join(takesDir, `${name}.mp4`))]));
const voAssets = new Map([...vo.keys()].map((lineId) => [lineId, importAsset(join(BUILD, "vo-norm", `${lineId}.m4a`))]));
const soundAssets = new Map(Object.keys(script.dictations).map((id) => [id, importAsset(join(BUILD, "dictation-norm", `${id}.m4a`))]));

// The script, with every line placed where its narration plays.
const placed = new Map(segments.flatMap((s) => s.lines).map((l) => [l.id, l]));
const scriptBody = {
  sections: script.sections.map((section, s) => ({
    title: section.title,
    intent: section.intent,
    status: "review",
    lines: section.lines.map((line, l) => {
      const p = placed.get(`${s + 1}.${l + 1}`)!;
      return { say: line.say, show: line.show, at: frames(p.at), duration: frames(p.duration) };
    }),
  })),
};
const scriptFile = join(BUILD, "neon", "stfu-script.json");
await mkdir(dirname(scriptFile), { recursive: true });
await writeFile(scriptFile, JSON.stringify(scriptBody, null, 2));
neon("script", "set", scriptFile);
type ScriptState = { sections: { lines: { id: string }[] }[] };
const scriptState = neon<ScriptState>("api", "GET", "script");
const lineIds = new Map(scriptState.sections.flatMap((section, s) => section.lines.map((line, l) => [`${s + 1}.${l + 1}`, line.id])));

type Op = { route: string; body: Record<string, unknown> };
const ops: Op[] = [];
const insert = (body: Record<string, unknown>) => {
  ops.push({ route: "/api/timeline/insert", body: { placement: "free", ...body } });
  return ops.length - 1;
};

for (const segment of segments) {
  const at = frames(segment.at);
  const duration = frames(segment.length);
  const { visual } = segment;
  if (visual.kind === "take") {
    const take = takes.get(visual.take)!;
    const op = insert({ kind: "video", assetId: takeAssets.get(visual.take), at, duration, trimBefore: frames(segment.trim), trackId: V1, name: visual.take });
    const inSegment = (t: number) => t >= segment.trim && t < segment.trim + segment.length;
    const global = (t: number) => frames(segment.at + t - segment.trim);
    for (const event of take.events) {
      if (event.kind === "zoom" && inSegment(event.from)) {
        ops.push({ route: "/api/timeline/zoom", body: { id: `$${op}.id`, from: global(event.from), to: Math.min(global(event.to), at + duration - 1), cx: event.center[0], cy: event.center[1], zoom: event.zoom, ramp: frames(0.45) } });
      } else if (event.kind === "keys" && inSegment(event.at)) {
        const hotkey = event.combo === "Fn" || event.combo === "Ctrl+Win";
        insert({ kind: "component", componentName: "KeyCombo", props: { combo: event.combo, platform: event.platform, label: hotkey ? "hold to talk" : "", x: 0.5, y: 0.13, size: 40, color: ACCENT }, at: global(event.at), duration: frames(event.duration), trackId: FX2 });
      } else if (event.kind === "callout" && inSegment(event.at)) {
        insert({ kind: "component", componentName: "Callout", props: { text: event.text, x: event.x, y: event.y, side: "auto", color: ACCENT, size: 30 }, at: global(event.at), duration: frames(event.duration), trackId: FX2 });
      } else if (event.kind === "sound" && inSegment(event.at)) {
        insert({ kind: "audio", assetId: soundAssets.get(event.clip), at: global(event.at), trackId: A2, name: `Dictation ${event.clip}` });
      }
    }
  } else {
    insert({ kind: "component", componentName: visual.fx.name, props: visual.fx.props, at, duration, trackId: FX1 });
  }
  for (const line of segment.lines) {
    insert({ kind: "audio", assetId: voAssets.get(line.id), at: frames(line.at), trackId: A1, scriptLineId: lineIds.get(line.id), name: `VO ${line.id}` });
  }
}

// Chapter labels where a section opens on footage.
for (const [s, section] of script.sections.entries()) {
  const first = placed.get(`${s + 1}.1`)!;
  if (visualFor(lines.find((l) => l.id === `${s + 1}.1`)!).kind !== "take") continue;
  insert({ kind: "component", componentName: "StfuChapter", props: { number: s + 1, title: section.title }, at: frames(first.at), duration: frames(3.5), trackId: FX2 });
}

const planFile = join(BUILD, "neon", "stfu-plan.json");
await writeFile(planFile, JSON.stringify({ ops }, null, 2));
neon("apply", planFile);
neon("project", "save");
process.stdout.write(`Applied ${ops.length} edits to ${PROJECT}\n`);

if (process.argv.includes("--render")) {
  const output = join(BUILD, "renders", "stfu.mp4");
  await mkdir(dirname(output), { recursive: true });
  neon("render", "--output", output, "--preset", "1080p30");
  process.stdout.write(`Rendered ${output}\n`);
}

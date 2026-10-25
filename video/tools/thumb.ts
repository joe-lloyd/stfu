// Builds the video's YouTube thumbnail with Neon's thumbnail feature and renders it.
//
//   node video/tools/thumb.ts     # after edit.ts → video/build/thumbs/stfu.png (1280x720)
//
// The background is the timeline partway through the line named in script.json's
// youtube.thumbFrom; StfuThumbnail from the FX pack draws the title and kicker over it.
import { mkdir, readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { neon, PROJECT } from "./neon.ts";

const VIDEO = join(dirname(fileURLToPath(import.meta.url)), "..");
const script: { youtube: { thumbTitle: string; thumbKicker: string; thumbFrom: string } } = JSON.parse(await readFile(join(VIDEO, "script.json"), "utf8"));

neon("project", "open", PROJECT);
neon("packs", "install", join(VIDEO, "fx", "stfu-fx"));
const state = neon<{ layers: { id: string }[] }>("api", "GET", "thumbnail");
if (state.layers.length) neon("thumbnail", "remove", ...state.layers.map((l) => l.id));
const [s, l] = script.youtube.thumbFrom.split(".").map(Number);
type Placed = { sections: { lines: { placed?: { startFrame: number; durationFrames: number } }[] }[] };
const line = neon<Placed>("api", "GET", "script").sections[s! - 1]!.lines[l! - 1]!.placed!;
neon("thumbnail", "background", "--at", `${line.startFrame + Math.round(line.durationFrames * 0.7)}f`);
neon("thumbnail", "add", "--component", "StfuThumbnail", "--props", JSON.stringify({ title: script.youtube.thumbTitle, kicker: script.youtube.thumbKicker }), "--pos", "0.5,0.5", "--scale", "1");
const out = join(VIDEO, "build", "thumbs", "stfu.png");
await mkdir(dirname(out), { recursive: true });
neon("thumbnail", "render", "--out", out);
neon("project", "save");
process.stdout.write(`${out}\n`);

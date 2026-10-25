// Records the takes of the STFU tour: the real pill and settings pages on the stage desktop.
//
//   node video/tools/voice.mjs
//   node video/tools/record.ts [take...]      # no arguments records every take
//
// Starts the stage's Vite server unless it already runs, then plays each take in headless
// Chromium at 1280x720 CSS pixels and 1.5x, so frames come out at 1920x1080. A take lands in
// video/build/takes/<take>.mp4 plus <take>.json with each line's start and the edit's events.
import { spawn } from "node:child_process";
import { mkdir, readFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { Take, type VoLine } from "./director.ts";
import { takes, VIEWPORT } from "./takes.ts";

const VIDEO = join(dirname(fileURLToPath(import.meta.url)), "..");
const REPO = join(VIDEO, "..");
export const STAGE = "http://localhost:1430";
const CHROMIUM = process.env.CHROMIUM ?? join(homedir(), "Library/Caches/ms-playwright/chromium-1243/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");

const vo = new Map<string, VoLine>(JSON.parse(await readFile(join(VIDEO, "build", "vo", "manifest.json"), "utf8")).lines.map((l: VoLine) => [l.id, l]));

// Pre-flight: every t.cue("word") must name a word of its line, or the take dies minutes in.
const source = await readFile(join(VIDEO, "tools", "takes.ts"), "utf8");
const problems: string[] = [];
for (const block of source.split(/\bt\.line\("/).slice(1)) {
  const lineId = block.slice(0, block.indexOf('"'));
  const line = vo.get(lineId);
  if (!line) {
    problems.push(`${lineId}: no such line`);
    continue;
  }
  const spoken = line.words.map((w) => w.word.toLowerCase().replace(/[^a-z0-9-]/g, ""));
  for (const [, word, nth] of block.matchAll(/\bt\.cue\("([^"]+)"(?:,\s*(\d+))?\)/g)) {
    if (spoken.filter((w) => w === word!.toLowerCase()).length < Number(nth ?? 1)) problems.push(`${lineId}: no "${word}" in ${spoken.join(" ")}`);
  }
}
if (problems.length) throw new Error(`cues:\n  ${problems.join("\n  ")}`);

async function stageUp(): Promise<boolean> {
  return fetch(`${STAGE}/video/stage/desk.html`).then((r) => r.ok, () => false);
}
let server: ReturnType<typeof spawn> | undefined;
if (!(await stageUp())) {
  server = spawn("pnpm", ["exec", "vite", "--config", "video/stage/vite.config.mjs"], { cwd: REPO, stdio: "ignore" });
  for (let i = 0; i < 60 && !(await stageUp()); i++) await new Promise((r) => setTimeout(r, 250));
}

const out = join(VIDEO, "build", "takes");
await mkdir(out, { recursive: true });
const browser = await chromium.launch({ executablePath: CHROMIUM, headless: true, args: ["--autoplay-policy=no-user-gesture-required", "--hide-scrollbars"] });
const wanted = process.argv.slice(2);
try {
  for (const [name, fn] of Object.entries(takes)) {
    if (wanted.length && !wanted.includes(name)) continue;
    process.stdout.write(`▶ ${name}\n`);
    const context = await browser.newContext({ viewport: VIEWPORT, deviceScaleFactor: 1.5 });
    const page = await context.newPage();
    page.on("pageerror", (e) => process.stdout.write(`  page error: ${e.message}\n`));
    await page.goto(`${STAGE}/video/stage/desk.html`);
    await page.waitForFunction(() => "stage" in window);
    await page.waitForTimeout(600);
    try {
      await fn({ page, take: async () => {
        const take = new Take(name, page, vo, { outDir: out, viewport: VIEWPORT });
        await take.start();
        return take;
      } });
    } catch (error) {
      await mkdir(join(VIDEO, "build", "debug"), { recursive: true });
      await page.screenshot({ path: join(VIDEO, "build", "debug", `${name}.png`) }).catch(() => {});
      throw error;
    } finally {
      await context.close();
    }
  }
} finally {
  await browser.close();
  server?.kill();
}

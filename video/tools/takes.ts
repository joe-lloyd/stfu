// The takes of the STFU tour, one function per take. Each acts out its script lines on the stage
// desktop (video/stage/desk.html) with the cursor, times actions to the narration's words, and
// records zooms, key combos and sounds for the edit. Dictations happen between lines, so the
// narrator and the person dictating never talk over each other.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { Frame, Locator, Page } from "playwright-core";
import type { Take } from "./director.ts";

export const VIEWPORT = { width: 1280, height: 720 };
const VIDEO = join(dirname(fileURLToPath(import.meta.url)), "..");

type Dictation = { voice: string; say: string; cleaned: string; app: string };
const script: { dictations: Record<string, Dictation> } = JSON.parse(readFileSync(join(VIDEO, "script.json"), "utf8"));
const meta = (id: string): { duration: number; levels: number[] } => JSON.parse(readFileSync(join(VIDEO, "build", "dictation", `${id}.json`), "utf8"));

export type Ctx = { page: Page; take: () => Promise<Take> };

type StageFn = (...args: unknown[]) => unknown;
function stage(page: Page, fn: string, ...args: unknown[]): Promise<unknown> {
  return page.evaluate(({ fn, args }) => (window as unknown as { stage: Record<string, StageFn> }).stage[fn]!(...args), { fn, args });
}

function seed(page: Page, ids: string[]) {
  return stage(page, "seed", ids.map((id) => ({ id, raw: script.dictations[id]!.say, cleaned: script.dictations[id]!.cleaned, app: script.dictations[id]!.app, seconds: meta(id).duration })));
}

/** Seconds of key-down before the speech starts. */
const LEAD = 0.35;

/** Hold the hotkey, say dictation `id`, release: the pill records, processes and pastes. */
async function dictate(t: Take, id: string, platform: "mac" | "pc" = "mac", zoom?: { center: [number, number]; zoom: number; extra?: number }) {
  const d = script.dictations[id]!;
  const m = meta(id);
  t.keys(platform === "mac" ? "Fn" : "Ctrl+Win", platform, LEAD + m.duration + 0.25);
  t.sound(id, LEAD);
  if (zoom) t.zoom(zoom.center, LEAD + m.duration + 0.3 + (zoom.extra ?? 0), zoom.zoom);
  await stage(t.page, "dictate", { id, raw: d.say, cleaned: d.cleaned, app: d.app, levels: m.levels, lead: LEAD });
}

/** The pill sits bottom centre: 54 px up on the Mac stage, 62 px on Windows (above the taskbar). */
const PILL_MAC: [number, number] = [0.5, 0.8];
const PILL_WIN: [number, number] = [0.5, 0.78];

async function zoomOn(t: Take, target: Locator, seconds: number, zoom = 1.6, nudge: [number, number] = [0, 0]) {
  const box = await target.boundingBox();
  if (!box) throw new Error("zoom target has no box");
  const x = (box.x + box.width / 2) / VIEWPORT.width + nudge[0];
  const y = (box.y + box.height / 2) / VIEWPORT.height + nudge[1];
  t.zoom([x, y], seconds, zoom);
}

/** A callout pointing at the middle of `target`. */
async function calloutOn(t: Take, target: Locator, text: string, seconds: number) {
  const box = await target.boundingBox();
  if (!box) throw new Error("callout target has no box");
  t.callout(text, (box.x + box.width / 2) / VIEWPORT.width, (box.y + box.height / 2) / VIEWPORT.height, seconds);
}

function settingsFrame(page: Page): Frame {
  const frame = page.frames().find((f) => f.url().includes("settings.html"));
  if (!frame) throw new Error("settings is not open");
  return frame;
}
const sf = (page: Page) => page.frameLocator("#settings-frame");

/** Smoothly scrolls the settings window so `selector` sits at the top (or `block`). */
async function scrollSettings(page: Page, selector: string, block: "start" | "center" | "end" = "start", ms = 1000) {
  await settingsFrame(page).evaluate(({ selector, block }) => document.querySelector(selector)!.scrollIntoView({ behavior: "smooth", block }), { selector, block });
  await page.waitForTimeout(ms);
}

async function openSettings(t: Take) {
  const { page } = t;
  await t.click(page.locator("#tray"));
  await stage(page, "tray", true);
  const item = page.locator('#traymenu [data-item="settings"]');
  await t.moveTo(item, 450);
  await stage(page, "trayHighlight", "settings");
  await t.click(item);
  await stage(page, "tray", false);
  await stage(page, "show", "win-settings");
  await sf(page).locator("#perm-list .perm").first().waitFor();
  await page.waitForTimeout(300);
}

/** Opens settings before the take starts, already loaded. */
async function settingsReady(page: Page) {
  await stage(page, "show", "win-settings");
  await sf(page).locator("#perm-list .perm").first().waitFor();
  await page.waitForTimeout(500);
}

export const takes: Record<string, (ctx: Ctx) => Promise<void>> = {
  async open({ page, take }) {
    await stage(page, "show", "win-mail");
    const t = await take();
    const body = page.locator("#mail-body");
    await t.line("1.2", async () => {
      await t.cue("type");
      await t.moveTo({ x: 520, y: 300 }, 900);
    });
    await t.line("1.3", async () => {
      await t.cue("click");
      await t.click(body);
      await stage(page, "caret", "mail-body");
    });
    await dictate(t, "d1", "mac", { center: PILL_MAC, zoom: 1.8 });
    await t.line("1.4", async () => {
      t.zoom([0.36, 0.42], 8.5, 1.7);
    });
    await t.finish();
  },

  async rules({ page, take }) {
    await stage(page, "show", "win-notes");
    const t = await take();
    await t.line("3.1", async () => {
      await t.cue("notes");
      await t.click(page.locator("#notes-body"));
      await stage(page, "caret", "notes-body");
    });
    await dictate(t, "d2", "mac", { center: PILL_MAC, zoom: 1.8 });
    await t.line("3.2", async () => {
      t.zoom([0.48, 0.33], 4.6, 1.7);
    });
    await t.line("3.3", async () => {
      await t.cue("chat");
      await stage(page, "show", "win-chat");
      await t.cue("reply");
      await t.click(page.locator("#chat-input"));
    });
    await dictate(t, "d3", "mac", { center: [0.55, 0.78], zoom: 1.5 });
    await t.pause(0.4);
    await stage(page, "chatSend");
    t.keys("Enter", "mac", 1);
    await t.line("3.4", async () => {
      t.zoom([0.6, 0.62], 4.5, 1.6);
    });
    await t.finish();
  },

  async settings({ page, take }) {
    await stage(page, "show", "win-mail");
    await seed(page, ["d1", "d2", "d3"]);
    const t = await take();
    const s = sf(page);
    await t.line("4.1", async () => {
      await t.cue("opens");
      await openSettings(t);
    });
    await t.line("4.2", async () => {
      await scrollSettings(page, "#perm-section", "start", 900);
      await zoomOn(t, s.locator("#perm-section"), 11.5, 1.6);
      await t.cue("microphone");
      await t.moveTo(s.locator('.perm[data-key="microphone"] .name'), 400);
      await t.cue("input");
      await t.moveTo(s.locator('.perm[data-key="input_monitoring"] .name'), 400);
      await t.cue("accessibility");
      await t.moveTo(s.locator('.perm[data-key="accessibility"] .name'), 400);
    });
    await t.line("4.3", async () => {
      await scrollSettings(page, "#stt-provider", "center", 900);
      await t.cue("grock");
      await t.moveTo(s.locator("#stt-provider"));
      await calloutOn(t, s.locator("#stt-provider"), "Groq · OpenAI · whisper.cpp · Speaches", 7);
    });
    await t.line("4.4", async () => {
      await t.cue("test");
      await t.click(s.locator("#stt-test"));
      await zoomOn(t, s.locator("#stt-key"), 5, 1.7, [0, 0.04]);
    });
    await t.line("4.5", async () => {
      await scrollSettings(page, "#llm-provider", "center", 900);
      await t.cue("kwen");
      await t.moveTo(s.locator("#llm-model"));
      await zoomOn(t, s.locator("#llm-model"), 3.5, 1.7);
    });
    await t.line("4.6", async () => {
      await t.cue("opencode");
      await t.click(s.locator("#llm-provider"));
      // A new user's key field is empty until Connect OpenCode fills it.
      await s.locator("#llm-key").fill("");
      await s.locator("#llm-provider").selectOption("zen");
      await t.cue("connect");
      await t.click(s.locator("#zen-connect-btn"));
      await zoomOn(t, s.locator("#zen-connect"), 3.8, 1.6, [0, 0.06]);
      await t.cue("free");
      await calloutOn(t, s.locator("#llm-model-select"), "10 models from Zen, free ones first", 4);
    });
    await t.line("4.7", async () => {
      await t.cue("wrong");
      const key = s.locator("#llm-key");
      await t.click(key);
      await key.selectText();
      await key.pressSequentially("sk-zen-2025-expired", { delay: 40 });
      await key.press("Tab");
      await zoomOn(t, s.locator("#zen-status"), 4, 1.6, [-0.05, 0]);
    });
    await t.line("4.8", async () => {
      await t.cue("oh-llama");
      await t.click(s.locator("#llm-provider"));
      await s.locator("#llm-provider").selectOption("ollama");
      await page.waitForTimeout(500);
      await scrollSettings(page, "#llm-status", "center", 800);
      await zoomOn(t, s.locator("#llm-status"), 5, 1.6, [0.1, -0.06]);
    });
    await t.line("4.9", async () => {
      await scrollSettings(page, "#profile-bar", "center", 900);
      await t.cue("local");
      await t.click(s.locator("#profile-select"));
      await s.locator("#profile-select").selectOption("Local (offline)");
      await t.cue("switch");
      await scrollSettings(page, "#stt-status", "center", 1000);
      await zoomOn(t, s.locator("#stt-provider"), 3, 1.5, [0, 0.05]);
    });
    await t.finish();
  },

  async clipboard({ page, take }) {
    await seed(page, ["d1", "d2", "d3"]);
    const t = await take();
    await t.line("5.1", async () => {
      await t.cue("nothing");
      await t.click(page.locator("#wall"));
    });
    await dictate(t, "d4", "mac", { center: PILL_MAC, zoom: 2, extra: 3 });
    await t.line("5.2", async () => {
      await t.cue("paste");
      await stage(page, "show", "win-notes");
      await t.click(page.locator("#notes-body"));
      await stage(page, "caret", "notes-body");
      t.keys("Cmd+V", "mac", 1.2);
      await stage(page, "pasteClipboard");
      t.zoom([0.48, 0.28], 3, 1.6);
    });
    await t.line("5.3", async () => {
      await t.cue("top");
      await stage(page, "show", "win-settings");
      await sf(page).locator("#last-text").waitFor();
      await page.waitForTimeout(300);
      await zoomOn(t, sf(page).locator("#last-section"), 3.2, 1.5, [0, -0.08]);
      await t.cue("menu");
      await t.click(page.locator("#tray"));
      await stage(page, "tray", true);
      await t.moveTo(page.locator('#traymenu [data-item="copy-last"]'), 400);
      await stage(page, "trayHighlight", "copy-last");
      await t.cue("choose");
      await stage(page, "tray", false);
      await t.moveTo(sf(page).locator("#clipboard-mode"));
      await calloutOn(t, sf(page).locator("#clipboard-mode"), "When there's no text field · Always · Never", 3);
    });
    await t.finish();
  },

  async history({ page, take }) {
    await seed(page, ["d1", "d2", "d3"]);
    await settingsReady(page);
    const t = await take();
    const s = sf(page);
    await t.line("6.1", async () => {
      await t.cue("history");
      await t.click(s.locator("#tab-btn-history"));
      await t.cue("file");
      await scrollSettings(page, ".hist-bar", "start", 1000);
    });
    const first = s.locator("#hist-list .entry").first();
    await t.line("6.2", async () => {
      await zoomOn(t, first, 5, 1.5);
      await t.cue("pasted");
      await t.moveTo(first.locator(".text"), 500);
      await t.cue("app");
      await t.moveTo(first.locator(".meta span").nth(2), 500);
      await t.cue("long");
      await t.moveTo(first.locator(".meta span").last(), 500);
    });
    await t.line("6.3", async () => {
      await t.click(first.locator("summary"));
      await page.waitForTimeout(400);
      await t.cue("raw");
      await settingsFrame(page).evaluate(() => document.scrollingElement!.scrollBy({ top: 260, behavior: "smooth" }));
      await t.cue("sent");
      await settingsFrame(page).evaluate(() => document.scrollingElement!.scrollBy({ top: 300, behavior: "smooth" }));
      await t.cue("paste");
      await settingsFrame(page).evaluate(() => document.scrollingElement!.scrollBy({ top: 420, behavior: "smooth" }));
    });
    await t.line("6.4", async () => {
      await scrollSettings(page, ".hist-bar", "start", 900);
      await t.cue("audio");
      await t.moveTo(first.locator("button[data-play]"), 400);
      await t.cue("raw");
      await t.moveTo(first.locator("button", { hasText: "Copy raw transcript" }), 400);
      await t.cue("record");
      await t.moveTo(first.locator("button", { hasText: "Copy JSON" }), 400);
    });
    // Playback after the line, so the dictation's voice never talks over the narration.
    await t.click(first.locator("button[data-play]"));
    t.sound("d3", 0.05);
    await t.pause(meta("d3").duration + 0.4);
    await t.pause(0.6);
    await t.line("6.5", async () => {
      await scrollSettings(page, "#tab-history section:nth-of-type(2)", "start", 1000);
      await zoomOn(t, s.locator(".limits"), 7, 1.6);
      await t.cue("limits");
      await t.moveTo(s.locator("#hist-text-days"), 500);
      await t.cue("recordings");
      await t.moveTo(s.locator("#hist-max"), 500);
    });
    await t.line("6.6", async () => {
      await t.cue("compressed");
      await t.click(s.locator("#hist-compress"));
      await zoomOn(t, s.locator("#tab-history section:nth-of-type(2)"), 6.5, 1.4, [0, 0.05]);
    });
    await t.line("6.7", async () => {
      await t.cue("ask");
      await t.moveTo(s.locator("#hist-confirm"), 500);
      await zoomOn(t, s.locator("#hist-confirm"), 4, 1.7, [0.1, 0]);
    });
    await t.finish();
  },

  async windows({ page, take }) {
    await stage(page, "os", "win");
    await stage(page, "show", "win-notepad");
    const t = await take();
    await t.line("7.1", async () => {
      await t.cue("control");
      await t.click(page.locator("#notepad-body"));
      await stage(page, "caret", "notepad-body");
    });
    await dictate(t, "d5", "pc", { center: [0.5, 0.72], zoom: 1.6 });
    await t.line("7.2", async () => {
      t.zoom([0.3, 0.27], 4.5, 1.7);
    });
    await t.finish();
  },

  async updates({ page, take }) {
    await seed(page, ["d1", "d2", "d3", "d4"]);
    await settingsReady(page);
    await scrollSettings(page, "#check-updates", "center", 1200);
    const s = sf(page);
    const t = await take();
    await t.line("8.1", async () => {
      await zoomOn(t, s.locator("#auto-update"), 10, 1.5, [0.1, 0.05]);
      await t.cue("six");
      await t.moveTo(s.locator("#auto-update"), 500);
    });
    await t.line("8.2", async () => {
      await t.cue("check");
      await t.click(s.locator("#check-updates"));
      await zoomOn(t, s.locator("#check-updates"), 7, 1.7, [0.12, 0]);
    });
    await t.line("8.3", async () => {
      await t.cue("login");
      await t.click(page.locator("#tray"));
      await stage(page, "tray", true);
      await t.moveTo(page.locator('#traymenu [data-item="autostart"]'), 450);
      await stage(page, "trayHighlight", "autostart");
      t.zoom([0.86, 0.17], 3.8, 1.8);
    });
    await t.finish();
  },
};

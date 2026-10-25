// The stage: a desktop with a few writing apps, STFU's real pill and settings pages in iframes,
// and the hotkey-to-paste sequence the Rust core runs (src-tauri/src/lib.rs), played here so the
// recording script can drive it. Exposed as window.stage.
import { backend } from "../web-shims/mock.ts";
import { emit } from "../web-shims/event.ts";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

// The backend lives on this, the top window, so its timers outlive the iframes.
backend();

// Faint waveform across the bottom of the wallpaper, like the app icon.
const bars = $("wallbars");
bars.innerHTML = Array.from({ length: 64 }, (_, i) => {
  const h = 30 + Math.abs(Math.sin(i * 0.37) * Math.cos(i * 0.11)) * 210;
  return `<rect x="${i * 20 + 4}" y="${260 - h}" width="12" height="${h + 10}" rx="6" fill="${i % 9 === 0 ? "#f5276c" : "#7dd3fc"}"/>`;
}).join("");

// The clocks show the real time, which is also what the history entries are stamped with.
const now = new Date();
const hm = now.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
$("clock").textContent = `${now.toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" }).replace(",", "")} ${hm}`;
$("winclock").innerHTML = `${hm}<br>${now.toLocaleDateString("en-GB")}`;

const pill = $<HTMLIFrameElement>("pill");
pill.src = "/index.html";
let settingsLoaded = false;

function front(id: string) {
  document.querySelectorAll<HTMLElement>(".window").forEach((w) => (w.style.zIndex = w.id === id ? "20" : "10"));
  $("appname").textContent = $(id).dataset.app ?? "Finder";
}

/** Where the text lands: the focused text field in a window, or nowhere. */
function target(): HTMLTextAreaElement | null {
  const el = document.activeElement;
  return el instanceof HTMLTextAreaElement ? el : null;
}

function insert(field: HTMLTextAreaElement, text: string) {
  field.focus();
  field.setRangeText(text, field.selectionStart, field.selectionEnd, "end");
}

let clipboard = "";

type Dictation = { id: string; raw: string; cleaned: string; app: string; bundle?: string; levels: number[]; lead: number };

const stage = {
  os(kind: "mac" | "win") {
    document.body.classList.toggle("win", kind === "win");
  },
  show(id: string) {
    const w = $(id);
    w.classList.add("shown");
    front(id);
    if (id === "win-settings" && !settingsLoaded) {
      settingsLoaded = true;
      $<HTMLIFrameElement>("settings-frame").src = "/settings.html";
    }
  },
  hide(id: string) {
    $(id).classList.remove("shown");
  },
  hideAll() {
    document.querySelectorAll(".window").forEach((w) => w.classList.remove("shown"));
    (document.activeElement as HTMLElement | null)?.blur();
    $("appname").textContent = "Finder";
  },
  /** Focus a text field and put the caret at its end. */
  caret(id: string) {
    const field = $<HTMLTextAreaElement>(id);
    field.focus();
    field.setSelectionRange(field.value.length, field.value.length);
  },
  tray(open: boolean) {
    const menu = $("traymenu");
    const icon = $("tray").getBoundingClientRect();
    menu.style.left = `${Math.min(icon.left - 8, 1280 - 244)}px`;
    menu.classList.toggle("open", open);
    $("tray").classList.toggle("open", open);
    if (!open) menu.querySelectorAll(".hl").forEach((i) => i.classList.remove("hl"));
  },
  trayHighlight(item: string) {
    document.querySelectorAll("#traymenu .item").forEach((i) => i.classList.toggle("hl", (i as HTMLElement).dataset.item === item));
  },
  /** Cmd+V by hand: the last dictation, left on the clipboard. */
  pasteClipboard() {
    const field = target();
    if (field && clipboard) insert(field, clipboard);
  },
  /**
   * One take, as the Rust core runs it: key down shows the pill and starts recording, levels
   * stream in while the key is held, release turns the wave yellow while speech-to-text and the
   * cleanup run, then the text is pasted at the caret, or left on the clipboard when no text
   * field has focus, and the pill fades out.
   */
  async dictate(d: Dictation): Promise<{ pasted: boolean }> {
    const field = target();
    pill.classList.add("shown");
    await emit("state", { phase: "recording" });
    const start = performance.now();
    // `lead` seconds of silence while the key goes down, then the speech's own loudness.
    const levels = [...Array(Math.round(d.lead * 30)).fill(0.004), ...d.levels, ...Array(8).fill(0.004)];
    await new Promise<void>((resolve) => {
      let sent = -1;
      const tick = () => {
        const i = Math.floor(((performance.now() - start) / 1000) * 30);
        while (sent < Math.min(i, levels.length - 1)) {
          sent++;
          void emit("level", levels[sent]);
        }
        if (i >= levels.length) resolve();
        else requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
    });
    await emit("state", { phase: "processing" });
    await sleep(900);
    const audioMs = Math.round((d.levels.length / 30) * 1000);
    if (field) {
      insert(field, d.cleaned);
      await emit("state", { phase: "done" });
    } else {
      clipboard = d.cleaned;
      await emit("state", { phase: "done", message: "Copied to clipboard" });
    }
    backend().dictated({ id: d.id, raw: d.raw, cleaned: d.cleaned, app: d.app, bundle: d.bundle, audioMs, onClipboard: !field });
    await sleep(field ? 1000 : 2200);
    pill.classList.remove("shown");
    return { pasted: !!field };
  },
  /** Dictations from earlier in the session, so Settings and History start where the video left off. */
  seed(list: { id: string; raw: string; cleaned: string; app: string; seconds: number }[]) {
    for (const d of list) backend().dictated({ id: d.id, raw: d.raw, cleaned: d.cleaned, app: d.app, audioMs: Math.round(d.seconds * 1000) });
  },
  /** Enter in the chat box: the message joins the channel as Joe's. */
  chatSend() {
    const input = $<HTMLTextAreaElement>("chat-input");
    const time = new Date().toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
    const msg = document.createElement("div");
    msg.className = "msg";
    msg.innerHTML = `<div class="av" style="background:#7dd3fc">J</div><div><div class="who">Joe Lloyd<span>${time}</span></div><div class="body"></div></div>`;
    msg.querySelector(".body")!.textContent = input.value.trim();
    $("chat-log").append(msg);
    input.value = "";
  },
  settings(): Window {
    return $<HTMLIFrameElement>("settings-frame").contentWindow!;
  },
};

(window as unknown as { stage: typeof stage }).stage = stage;

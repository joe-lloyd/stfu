import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// One line of history/<day>.jsonl. Fields are optional here because the file may hold lines
// written by an older or newer version of the app.
interface Rec {
  id: string;
  ts: string;
  profile?: string;
  language?: string;
  target_app?: { name: string; bundle_id?: string };
  outcome?: string;
  error?: string;
  audio?: { file?: string; held_ms?: number; duration_ms?: number; bytes?: number };
  stt?: { base_url?: string; api?: string; model?: string; duration_ms?: number; text?: string; response?: unknown; error?: string };
  llm?: {
    enabled?: boolean; base_url?: string; model?: string; wire?: string; duration_ms?: number;
    prompt?: string; reply?: string; cleaned?: string; error?: string; fell_back?: boolean;
  };
  output?: { text?: string; pasted?: boolean; paste_ms?: number; paste_error?: string };
  total_ms?: number;
}
interface Day { day: string; count: number }

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

let entries: Rec[] = [];
let playing: HTMLAudioElement | null = null;

function status(text: string, kind: "" | "ok" | "err" = "") {
  const el = $("hist-status");
  el.textContent = text;
  el.className = `status ${kind}`;
}

/** createElement with a class and text; never innerHTML, since dictated text can contain anything. */
function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls = "", text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
}

const secs = (ms?: number) => (ms === undefined ? "" : ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`);

function today(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

function dayLabel(d: Day): string {
  const label =
    d.day === today()
      ? "Today"
      : new Date(`${d.day}T12:00:00`).toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short", year: "numeric" });
  return `${label} · ${d.count}`;
}

async function copy(text: string, what: string) {
  try {
    await invoke("copy_text", { text });
    status(`Copied ${what}.`, "ok");
  } catch (e) {
    status(String(e), "err");
  }
}

async function play(rec: Rec, btn: HTMLButtonElement) {
  if (playing) {
    playing.pause();
    playing = null;
    document.querySelectorAll<HTMLButtonElement>("button[data-play]").forEach((b) => (b.textContent = "Play audio"));
    if (btn.dataset.playing === "1") { btn.dataset.playing = ""; return; }
  }
  try {
    const bytes = await invoke<ArrayBuffer>("history_audio", { id: rec.id });
    const url = URL.createObjectURL(new Blob([bytes], { type: "audio/wav" }));
    const audio = new Audio(url);
    playing = audio;
    btn.dataset.playing = "1";
    btn.textContent = "Stop";
    audio.onended = () => {
      URL.revokeObjectURL(url);
      btn.textContent = "Play audio";
      btn.dataset.playing = "";
      if (playing === audio) playing = null;
    };
    await audio.play();
  } catch (e) {
    status(String(e), "err");
  }
}

function stage(parent: HTMLElement, title: string, lines: [string, string | undefined][], body?: string) {
  const shown = lines.filter(([, v]) => v !== undefined && v !== "");
  if (!shown.length && !body) return;
  const box = el("div", "stage");
  box.append(el("h3", "", title));
  const text = [...shown.map(([k, v]) => `${k}: ${v}`), ...(body ? ["", body] : [])].join("\n");
  box.append(el("pre", "", text));
  parent.append(box);
}

function outcomeBadge(r: Rec): HTMLElement {
  const o = r.outcome || "unknown";
  const label = { pasted: "pasted", no_speech: "no speech", empty_cleanup: "empty", failed: "failed" }[o] ?? o;
  const cls = o === "pasted" ? (r.llm?.fell_back ? "warn" : "pasted") : o === "failed" ? "failed" : "";
  const b = el("span", `badge ${cls}`, r.llm?.fell_back && o === "pasted" ? "pasted raw" : label);
  if (r.llm?.fell_back) b.title = `Clean-up failed, the raw transcript was pasted: ${r.llm.error ?? ""}`;
  return b;
}

function card(r: Rec): HTMLElement {
  const box = el("div", "entry");

  const meta = el("div", "meta");
  const time = new Date(r.ts);
  meta.append(el("span", "time", isNaN(time.getTime()) ? r.ts : time.toLocaleTimeString()), outcomeBadge(r));
  if (r.target_app?.name) meta.append(el("span", "", `→ ${r.target_app.name}`));
  if (r.profile) meta.append(el("span", "", r.profile));
  const timing = [
    r.audio?.duration_ms !== undefined ? `${secs(r.audio.duration_ms)} audio` : "",
    r.stt?.duration_ms ? `stt ${secs(r.stt.duration_ms)}` : "",
    r.llm?.duration_ms ? `clean-up ${secs(r.llm.duration_ms)}` : "",
    r.total_ms ? `total ${secs(r.total_ms)}` : "",
  ].filter(Boolean).join(" · ");
  if (timing) meta.append(el("span", "", timing));
  box.append(meta);

  const output = r.output?.text || "";
  const raw = r.stt?.text || "";
  const shown = output || raw;
  box.append(el("div", shown ? "text" : "text muted", shown || r.error || "(nothing transcribed)"));

  const actions = el("div", "actions");
  const button = (label: string, fn: (b: HTMLButtonElement) => void) => {
    const b = el("button", "", label);
    b.addEventListener("click", () => fn(b));
    actions.append(b);
    return b;
  };
  if (output) button("Copy output", () => void copy(output, "the output"));
  if (raw) button("Copy raw transcript", () => void copy(raw, "the raw transcript"));
  if (r.audio?.file) button("Play audio", (b) => void play(r, b)).dataset.play = "1";
  button("Copy JSON", () => void copy(JSON.stringify(r, null, 2), "the full record"));
  box.append(actions);

  // Every stage, lazily built on first open so a long day renders fast.
  const details = el("details");
  details.append(el("summary", "", "All stages"));
  details.addEventListener("toggle", () => {
    if (!details.open || details.dataset.built) return;
    details.dataset.built = "1";
    stage(details, "1 · Recording", [
      ["held", secs(r.audio?.held_ms)],
      ["audio", secs(r.audio?.duration_ms)],
      ["size", r.audio?.bytes ? `${Math.round(r.audio.bytes / 1024)} KB` : undefined],
      ["file", r.audio?.file],
      ["language", r.language],
    ]);
    stage(
      details,
      "2 · Speech to text",
      [
        ["model", [r.stt?.model, r.stt?.api].filter(Boolean).join(" · ")],
        ["server", r.stt?.base_url],
        ["took", secs(r.stt?.duration_ms)],
        ["error", r.stt?.error],
      ],
      r.stt?.text,
    );
    if (r.stt?.response) stage(details, "2 · Server reply", [], JSON.stringify(r.stt.response, null, 2));
    if (r.llm?.enabled) {
      stage(details, "3 · Clean-up", [
        ["model", [r.llm.model, r.llm.wire].filter(Boolean).join(" · ")],
        ["server", r.llm.base_url],
        ["took", secs(r.llm.duration_ms)],
        ["error", r.llm.error],
        ["fell back to raw", r.llm.fell_back ? "yes" : undefined],
      ]);
      if (r.llm.prompt) stage(details, "3 · Sent to the model", [], r.llm.prompt);
      if (r.llm.reply) stage(details, "3 · Model reply", [], r.llm.reply);
      if (r.llm.cleaned) stage(details, "3 · Cleaned", [], r.llm.cleaned);
    } else if (r.llm) {
      stage(details, "3 · Clean-up", [["status", "off for this profile"]]);
    }
    stage(
      details,
      "4 · Paste",
      [
        ["into", r.target_app ? `${r.target_app.name}${r.target_app.bundle_id ? ` (${r.target_app.bundle_id})` : ""}` : undefined],
        ["pasted", r.output ? (r.output.pasted ? "yes" : "no") : undefined],
        ["took", secs(r.output?.paste_ms)],
        ["error", r.output?.paste_error ?? r.error],
      ],
      r.output?.text,
    );
  });
  box.append(details);
  return box;
}

function render() {
  const list = $("hist-list");
  list.replaceChildren();
  const q = $<HTMLInputElement>("hist-search").value.trim().toLowerCase();
  const shown = q ? entries.filter((r) => JSON.stringify(r).toLowerCase().includes(q)) : entries;
  if (!shown.length) {
    list.append(el("div", "empty", q ? "Nothing matches." : "No dictations yet. Hold the hotkey and talk: every take shows up here."));
    return;
  }
  list.append(...shown.map(card));
}

async function loadEntries() {
  const day = $<HTMLSelectElement>("hist-day").value;
  $<HTMLButtonElement>("hist-delete").disabled = !day;
  try {
    entries = day ? await invoke<Rec[]>("history_entries", { day }) : [];
    render();
  } catch (e) {
    status(String(e), "err");
  }
}

async function loadDays(keep = true) {
  const sel = $<HTMLSelectElement>("hist-day");
  const previous = sel.value;
  const days = await invoke<Day[]>("history_days");
  sel.innerHTML = "";
  for (const d of days) sel.append(new Option(dayLabel(d), d.day));
  if (!days.length) sel.append(new Option("No history yet", ""));
  if (keep && days.some((d) => d.day === previous)) sel.value = previous;
  await loadEntries();
}

async function saveToggles() {
  const enabled = $<HTMLInputElement>("hist-enabled").checked;
  $<HTMLInputElement>("hist-audio").disabled = !enabled;
  try {
    await invoke("history_settings", { enabled, saveAudio: $<HTMLInputElement>("hist-audio").checked });
    status(enabled ? "History is on." : "History is off. Nothing new will be written; existing files stay until you delete them.", "ok");
  } catch (e) {
    status(String(e), "err");
  }
}

let loaded = false;

/** Called when the History tab is shown. */
export async function showHistory() {
  if (!loaded) {
    loaded = true;
    const cfg = await invoke<{ history?: { enabled: boolean; save_audio: boolean } }>("get_config");
    $<HTMLInputElement>("hist-enabled").checked = cfg.history?.enabled ?? true;
    $<HTMLInputElement>("hist-audio").checked = cfg.history?.save_audio ?? true;
    $<HTMLInputElement>("hist-audio").disabled = !(cfg.history?.enabled ?? true);
    $("hist-path").textContent = (await invoke<string>("config_path")).replace(/config\.json$/, "history");
  }
  await loadDays(false);
}

$("hist-day").addEventListener("change", () => void loadEntries());
$("hist-search").addEventListener("input", render);
$("hist-enabled").addEventListener("change", () => void saveToggles());
$("hist-audio").addEventListener("change", () => void saveToggles());
$("hist-folder").addEventListener("click", () => invoke("history_open_folder").catch((e) => status(String(e), "err")));
$("hist-delete").addEventListener("click", async () => {
  const day = $<HTMLSelectElement>("hist-day").value;
  if (!day || !confirm(`Delete every dictation and recording from ${day}? This cannot be undone.`)) return;
  try {
    await invoke("history_delete_day", { day });
    status(`Deleted ${day}.`, "ok");
    await loadDays(false);
  } catch (e) {
    status(String(e), "err");
  }
});

// A dictation finished while this window is open: show it straight away.
listen("history-changed", () => {
  if (!loaded || $("tab-history").hidden) return;
  const sel = $<HTMLSelectElement>("hist-day");
  if (!sel.value || sel.value === today()) void loadDays(false);
  else void loadDays(true);
});

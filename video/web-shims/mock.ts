// The Rust side of STFU, played by the browser: answers every command the settings page and the
// pill invoke, with the state a few weeks of real use would leave behind. Values and messages
// follow src-tauri/src/commands.rs. One instance lives on the top window.
import { emit } from "./event.ts";

const GROQ = "https://api.groq.com/openai/v1";
const VERSION = "0.9.2";
const SUPPORT = "/Users/joe/Library/Application Support/stfu";
const MB = 1024 * 1024;

type HistoryConfig = { enabled: boolean; save_audio: boolean; audio_days: number; text_days: number; max_mb: number; confirm_cleanup: boolean };
type Rec = Record<string, unknown> & { id: string; ts: string; output?: { text?: string } };
export type Dictation = { id: string; raw: string; cleaned: string; app: string; bundle?: string; audioMs: number; onClipboard?: boolean };

const pad = (n: number) => String(n).padStart(2, "0");
const dayOf = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

const LANGUAGES = [
  ["", "Auto-detect"], ["en", "English"], ["nl", "Nederlands"], ["de", "Deutsch"], ["fr", "Français"], ["es", "Español"],
  ["it", "Italiano"], ["pt", "Português"], ["pl", "Polski"], ["sv", "Svenska"], ["da", "Dansk"], ["no", "Norsk"],
  ["fi", "Suomi"], ["tr", "Türkçe"], ["ru", "Русский"], ["uk", "Українська"], ["zh", "中文"], ["ja", "日本語"],
  ["ko", "한국어"], ["hi", "हिन्दी"], ["ar", "العربية"],
].map(([code, label]) => ({ code, label }));

/** What a Zen key lists. Ids follow Zen's naming: "-free" marks the free ones. */
const ZEN_MODELS = [
  { id: "big-pickle", free: true, wire: "chat" },
  { id: "grok-code-fast-1-free", free: true, wire: "chat" },
  { id: "kimi-k2-free", free: true, wire: "chat" },
  { id: "claude-haiku-4-5", free: false, wire: "messages" },
  { id: "claude-sonnet-4-5", free: false, wire: "messages" },
  { id: "deepseek-v3.2", free: false, wire: "chat" },
  { id: "gemini-3-flash", free: false, wire: "gemini" },
  { id: "glm-4.6", free: false, wire: "chat" },
  { id: "gpt-5-mini", free: false, wire: "responses" },
  { id: "qwen3-coder", free: false, wire: "chat" },
];
export const GOOD_ZEN_KEY = "sk-zen-7f3a9c21d84e";

function record(d: Dictation, ts: Date, profile = "Cloud (free)"): Rec {
  const stt = 240 + (d.audioMs % 170);
  const llm = 380 + (d.audioMs % 260);
  const prompt = `Transcript: """${d.raw}"""`;
  return {
    id: d.id,
    ts: ts.toISOString(),
    profile,
    language: "en",
    target_app: { name: d.app, bundle_id: d.bundle },
    outcome: "pasted",
    audio: { file: `audio/${dayOf(ts)}/${d.id}.wav`, held_ms: d.audioMs + 420, duration_ms: d.audioMs, bytes: Math.round((d.audioMs / 1000) * 32000) },
    stt: { base_url: GROQ, model: "whisper-large-v3-turbo", duration_ms: stt, text: d.raw, response: { text: d.raw, x_groq: { id: `req_01k${d.id.slice(-8)}` } } },
    llm: {
      enabled: true, base_url: GROQ, model: "qwen/qwen3.8-27b", wire: "chat", duration_ms: llm,
      prompt, reply: JSON.stringify({ original: d.raw, cleaned: d.cleaned }), cleaned: d.cleaned,
    },
    output: d.onClipboard
      ? { text: d.cleaned, pasted: false, focus: "no_text_field", on_clipboard: true }
      : { text: d.cleaned, pasted: true, paste_ms: 96 + (d.audioMs % 60), focus: "text_field" },
    total_ms: stt + llm + 140,
  };
}

/** Earlier takes from today and the weeks before, so the History tab looks lived in. */
function seedHistory(): Map<string, Rec[]> {
  const now = new Date();
  const earlier: [number, Dictation][] = [
    [190, { id: "t0915", raw: "um can you send me the, the slides from yesterday's demo when you get a sec", cleaned: "Can you send me the slides from yesterday's demo when you get a sec?", app: "Slack", bundle: "com.tinyspeck.slackmacgap", audioMs: 4300 }],
    [150, { id: "t0955", raw: "so the bug is in the uh the retry loop it never resets the counter after a success", cleaned: "The bug is in the retry loop: it never resets the counter after a success.", app: "Linear", bundle: "com.linear", audioMs: 5600 }],
    [95, { id: "t1050", raw: "commit message fix pill position on the second monitor", cleaned: "Fix pill position on the second monitor", app: "Terminal", bundle: "com.apple.Terminal", audioMs: 2900 }],
    [40, { id: "t1145", raw: "hey uh are we still on for lunch on friday", cleaned: "Hey, are we still on for lunch on Friday?", app: "Messages", bundle: "com.apple.MobileSMS", audioMs: 2400 }],
  ];
  const days = new Map<string, Rec[]>();
  days.set(dayOf(now), earlier.map(([minutesAgo, d]) => record(d, new Date(now.getTime() - minutesAgo * 60_000))).reverse());
  const samples = [
    ["ok so the plan for this week is uh finish the history tab and then the release", "OK, so the plan for this week is to finish the history tab and then the release.", "Notes"],
    ["reply to anna thanks for the review I'll fix the naming tonight", "Reply to Anna: thanks for the review, I'll fix the naming tonight.", "Mail"],
    ["can we uh can we skip the sync tomorrow", "Can we skip the sync tomorrow?", "Slack"],
  ];
  for (let back = 1; back <= 23; back++) {
    if (back % 6 === 0) continue;
    const date = new Date(now.getTime() - back * 86_400_000);
    date.setHours(10, 12, 0, 0);
    const count = 3 + ((back * 7) % 9);
    const recs: Rec[] = [];
    for (let i = 0; i < count; i++) {
      const [raw, cleaned, app] = samples[(back + i) % samples.length]!;
      recs.push(record({ id: `d${back}-${i}`, raw: raw!, cleaned: cleaned!, app: app!, audioMs: 2000 + ((back * 13 + i * 7) % 40) * 150 }, new Date(date.getTime() + i * 47 * 60_000)));
    }
    days.set(dayOf(date), recs.reverse());
  }
  return days;
}

function create() {
  const history = seedHistory();
  let cfg = {
    hotkey: ["Function"],
    active_profile: "Cloud (free)",
    profiles: [
      {
        name: "Cloud (free)",
        stt: { base_url: GROQ, api: null, model: "whisper-large-v3-turbo", api_key: "gsk_9TqL2vXw8mRk4nYz7cPb1sHd", language: "en" },
        llm: { enabled: true, base_url: GROQ, model: "qwen/qwen3.8-27b", api_key: "gsk_9TqL2vXw8mRk4nYz7cPb1sHd", timeout_secs: 8, wire: null },
      },
      {
        name: "Local (offline)",
        stt: { base_url: "http://localhost:8080", api: "whispercpp", model: "", api_key: "local", language: "en" },
        llm: { enabled: true, base_url: "http://localhost:11434/v1", model: "llama3.2:3b", api_key: "ollama", timeout_secs: 8, wire: null },
      },
    ],
    launch_at_login: true,
    auto_update: true,
    history: { enabled: true, save_audio: true, audio_days: 30, text_days: 0, max_mb: 500, confirm_cleanup: true } as HistoryConfig,
    clipboard: "when_no_text_field",
  };
  let last: { text: string; ts: string; app: string | null } | null = null;
  // Recordings before compression: 82% of the 500 MB cap, most of it older than today.
  let audio = 411 * MB;
  let compressible = 368 * MB;
  let compressing = false;
  const text = 2.4 * MB;

  const status = () => {
    const days = [...history.keys()].sort();
    return {
      path: `${SUPPORT}/history`,
      usage: { total_bytes: audio + text, audio_bytes: audio, text_bytes: text, days: days.length, oldest_day: days[0] ?? null, compressible_bytes: compressible },
      plan: { expired_days: [], audio_days: [], frees_bytes: 0 },
      near_limit: audio >= 0.8 * cfg.history.max_mb * MB,
      compressing,
    };
  };

  const commands: Record<string, (args: Record<string, unknown>) => unknown> = {
    get_config: () => structuredClone(cfg),
    config_path: () => `${SUPPORT}/config.json`,
    save_config: (a) => { cfg = structuredClone(a.cfg) as typeof cfg; },
    languages: () => LANGUAGES,
    app_version: () => VERSION,
    last_dictation: () => last,
    take_history_prompt: () => false,
    permission_status: () => [
      { key: "microphone", label: "Microphone", granted: true, detail: "", can_prompt: false },
      { key: "input_monitoring", label: "Input Monitoring", granted: true, detail: "", can_prompt: false },
      { key: "accessibility", label: "Accessibility", granted: true, detail: "", can_prompt: false },
    ],
    request_permission: () => undefined,
    open_url: () => undefined,
    copy_text: () => undefined,
    set_clipboard_mode: (a) => { cfg.clipboard = a.mode as string; },
    test_stt: async (a) => { await sleep(420); return `Works. ${(a.cfg as typeof cfg).profiles[0]!.stt.model} responded in 312 ms.`; },
    test_llm: async () => { await sleep(700); return "Works in 644 ms: “This is a test of the cleanup model.”"; },
    probe_local: async () => { await sleep(150); return { ollama_up: true, ollama_models: ["llama3.2:3b", "qwen3.5:4b", "gemma3:4b"], whisper_up: true }; },
    import_opencode_key: async () => { await sleep(500); return GOOD_ZEN_KEY; },
    zen_models: async (a) => {
      await sleep(650);
      if (a.apiKey !== GOOD_ZEN_KEY) throw "OpenCode rejected this key. Copy a new one from opencode.ai/zen.";
      return ZEN_MODELS;
    },
    check_for_updates: async () => { await sleep(1400); return `Up to date (version ${VERSION}).`; },
    history_days: () => [...history.entries()].map(([day, recs]) => ({ day, count: recs.length })).sort((a, b) => b.day.localeCompare(a.day)),
    history_entries: (a) => history.get(a.day as string) ?? [],
    history_audio: async (a) => {
      // The dictations shown in the video play their own voice; older takes play a short clip.
      const clip = /^d\d$/.test(a.id as string) ? (a.id as string) : "d1";
      return (await fetch(`/video/build/dictation/${clip}.mp3`)).arrayBuffer();
    },
    history_status: () => status(),
    history_settings: (a) => { cfg.history = a.history as HistoryConfig; return status(); },
    history_compress: () => {
      compressing = true;
      // FLAC lands at about half the size, measured on real takes.
      setTimeout(() => {
        audio -= compressible * 0.52;
        compressible = 0;
        compressing = false;
        void emit("history-changed");
      }, 2600);
      return status();
    },
    history_cleanup: () => status(),
    history_open_folder: () => undefined,
    history_delete_day: () => undefined,
    history_delete_audio: () => undefined,
  };

  return {
    async invoke(command: string, args: Record<string, unknown>) {
      const fn = commands[command];
      if (!fn) throw new Error(`mock backend: no command ${command}`);
      return fn(args);
    },
    /** A dictation finished on the stage: write it to history and tell the pages. */
    dictated(d: Dictation) {
      const now = new Date();
      const recs = history.get(dayOf(now)) ?? [];
      recs.unshift(record(d, now));
      history.set(dayOf(now), recs);
      audio += Math.round((d.audioMs / 1000) * 32000);
      last = { text: d.cleaned, ts: now.toISOString(), app: d.app };
      void emit("last-dictation", last);
      void emit("history-changed");
    },
    config: () => cfg,
  };
}

export type Backend = ReturnType<typeof create>;

export function backend(): Backend {
  const top = window.top as unknown as { __stfuBackend?: Backend };
  top.__stfuBackend ??= create();
  return top.__stfuBackend;
}

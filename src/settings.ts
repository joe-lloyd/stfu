import { invoke } from "@tauri-apps/api/core";

interface SttConfig { base_url: string; model: string; api_key: string; language: string }
interface LlmConfig { enabled: boolean; base_url: string; model: string; api_key: string; timeout_secs: number }
interface Config { hotkey: string[]; stt: SttConfig; llm: LlmConfig }

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const STT_PRESETS: Record<string, { url: string; model: string; keyUrl: string }> = {
  groq: { url: "https://api.groq.com/openai/v1", model: "whisper-large-v3-turbo", keyUrl: "https://console.groq.com/keys" },
  openai: { url: "https://api.openai.com/v1", model: "whisper-1", keyUrl: "https://platform.openai.com/api-keys" },
  custom: { url: "", model: "", keyUrl: "" },
};

const LLM_PRESETS: Record<string, { url: string; models: string[]; keyUrl: string; hint: string }> = {
  zen: {
    url: "https://opencode.ai/zen/v1",
    models: ["big-pickle", "nemotron-3.5-lightning-free", "ling-3.0-flash-fin-free", "mimo-v2.5-free", "union-alpha", "nemotron-3-ultra-free", "muse-spark-1.3-contributor-free"],
    keyUrl: "https://opencode.ai/zen",
    hint: "Free models are promotional and may disappear. Sign in at opencode.ai/zen and copy the API key.",
  },
  openai: { url: "https://api.openai.com/v1", models: ["gpt-5-mini", "gpt-5"], keyUrl: "https://platform.openai.com/api-keys", hint: "" },
  groq: { url: "https://api.groq.com/openai/v1", models: ["llama-3.3-70b-versatile", "llama-3.1-8b-instant"], keyUrl: "https://console.groq.com/keys", hint: "Same Groq key as speech-to-text works here." },
  ollama: { url: "http://localhost:11434/v1", models: ["llama3.2", "qwen2.5"], keyUrl: "https://ollama.com/download", hint: "Runs locally. Any non-empty key is accepted; use \"ollama\"." },
  custom: { url: "", models: [], keyUrl: "", hint: "" },
};

function detect<T extends { url: string }>(presets: Record<string, T>, url: string): string {
  const hit = Object.entries(presets).find(([k, p]) => k !== "custom" && p.url && p.url === url.replace(/\/+$/, ""));
  return hit ? hit[0] : "custom";
}

let cfg: Config;

function setStatus(id: string, text: string, kind: "" | "ok" | "err" = "") {
  const el = $(id);
  el.textContent = text;
  el.className = `status ${kind}`;
}

function applySttProvider(p: string) {
  const preset = STT_PRESETS[p];
  $("stt-custom").hidden = p !== "custom";
  if (p !== "custom") {
    $<HTMLInputElement>("stt-url").value = preset.url;
    $<HTMLInputElement>("stt-model").value = preset.model;
  }
  $<HTMLButtonElement>("stt-getkey").hidden = !preset.keyUrl;
}

function applyLlmProvider(p: string) {
  const preset = LLM_PRESETS[p];
  $("llm-custom").hidden = p !== "custom";
  if (p !== "custom") $<HTMLInputElement>("llm-url").value = preset.url;
  const list = $<HTMLDataListElement>("llm-models");
  list.innerHTML = preset.models.map((m) => `<option value="${m}">`).join("");
  const modelInput = $<HTMLInputElement>("llm-model");
  if (!preset.models.includes(modelInput.value) && preset.models.length) modelInput.value = preset.models[0];
  $("llm-hint").textContent = preset.hint;
  $<HTMLButtonElement>("llm-getkey").hidden = !preset.keyUrl;
  if (p === "ollama" && !$<HTMLInputElement>("llm-key").value) $<HTMLInputElement>("llm-key").value = "ollama";
}

function collect(): Config {
  return {
    hotkey: cfg.hotkey,
    stt: {
      base_url: $<HTMLInputElement>("stt-url").value.trim(),
      model: $<HTMLInputElement>("stt-model").value.trim(),
      api_key: $<HTMLInputElement>("stt-key").value.trim(),
      language: cfg.stt.language,
    },
    llm: {
      enabled: $<HTMLInputElement>("llm-enabled").checked,
      base_url: $<HTMLInputElement>("llm-url").value.trim(),
      model: $<HTMLInputElement>("llm-model").value.trim(),
      api_key: $<HTMLInputElement>("llm-key").value.trim(),
      timeout_secs: cfg.llm.timeout_secs,
    },
  };
}

async function load() {
  cfg = await invoke<Config>("get_config");
  $("cfgpath").textContent = await invoke<string>("config_path");
  $("hotkey").textContent = cfg.hotkey.join(" + ");

  const sttP = detect(STT_PRESETS, cfg.stt.base_url);
  $<HTMLSelectElement>("stt-provider").value = sttP;
  $<HTMLInputElement>("stt-url").value = cfg.stt.base_url;
  $<HTMLInputElement>("stt-model").value = cfg.stt.model;
  $<HTMLInputElement>("stt-key").value = cfg.stt.api_key;
  applySttProvider(sttP);
  if (sttP === "custom") { $<HTMLInputElement>("stt-url").value = cfg.stt.base_url; $<HTMLInputElement>("stt-model").value = cfg.stt.model; }

  const llmP = detect(LLM_PRESETS, cfg.llm.base_url);
  $<HTMLSelectElement>("llm-provider").value = llmP;
  $<HTMLInputElement>("llm-enabled").checked = cfg.llm.enabled;
  $<HTMLInputElement>("llm-url").value = cfg.llm.base_url;
  $<HTMLInputElement>("llm-model").value = cfg.llm.model;
  $<HTMLInputElement>("llm-key").value = cfg.llm.api_key;
  applyLlmProvider(llmP);
  $<HTMLInputElement>("llm-model").value = cfg.llm.model;

  if (!cfg.stt.api_key) setStatus("stt-status", "No key yet. Click “Get a key”, paste it here, then Test.");
  if (!cfg.llm.api_key && cfg.llm.enabled) setStatus("llm-status", "No key yet. Click “Get a key”, paste it here, then Test.");
}

function toggleShow(inputId: string) {
  const el = $<HTMLInputElement>(inputId);
  el.type = el.type === "password" ? "text" : "password";
}

async function test(kind: "stt" | "llm") {
  const btn = $<HTMLButtonElement>(`${kind}-test`);
  btn.disabled = true;
  setStatus(`${kind}-status`, "Testing…");
  try {
    const msg = await invoke<string>(kind === "stt" ? "test_stt" : "test_llm", { cfg: collect() });
    setStatus(`${kind}-status`, msg, "ok");
  } catch (e) {
    setStatus(`${kind}-status`, String(e), "err");
  } finally {
    btn.disabled = false;
  }
}

async function save() {
  try {
    cfg = collect();
    await invoke("save_config", { cfg });
    setStatus("save-status", "Saved. Hold the hotkey and talk.", "ok");
  } catch (e) {
    setStatus("save-status", String(e), "err");
  }
}

$("stt-provider").addEventListener("change", (e) => applySttProvider((e.target as HTMLSelectElement).value));
$("llm-provider").addEventListener("change", (e) => applyLlmProvider((e.target as HTMLSelectElement).value));
$("stt-show").addEventListener("click", () => toggleShow("stt-key"));
$("llm-show").addEventListener("click", () => toggleShow("llm-key"));
$("stt-test").addEventListener("click", () => test("stt"));
$("llm-test").addEventListener("click", () => test("llm"));
$("save").addEventListener("click", save);
$("stt-getkey").addEventListener("click", () => invoke("open_url", { url: STT_PRESETS[$<HTMLSelectElement>("stt-provider").value].keyUrl }));
$("llm-getkey").addEventListener("click", () => invoke("open_url", { url: LLM_PRESETS[$<HTMLSelectElement>("llm-provider").value].keyUrl }));
// Save on Cmd/Ctrl+S too.
window.addEventListener("keydown", (e) => { if ((e.metaKey || e.ctrlKey) && e.key === "s") { e.preventDefault(); save(); } });

load().catch((e) => setStatus("save-status", `Could not load config: ${e}`, "err"));

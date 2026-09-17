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

interface ZenModel { id: string; free: boolean; wire: string }
interface Language { code: string; label: string }
interface Permission { key: string; label: string; granted: boolean; detail: string; can_prompt: boolean }

const LLM_PRESETS: Record<string, { url: string; models: string[]; keyUrl: string; hint: string }> = {
  zen: {
    url: "https://opencode.ai/zen/v1",
    models: [],
    keyUrl: "https://opencode.ai/zen",
    hint: "One OpenCode account, every model they host. Each model is routed to its native endpoint automatically. Free-tier models may require being signed in to OpenCode's own app; paid ones bill your OpenCode account.",
  },
  openai: { url: "https://api.openai.com/v1", models: ["gpt-5-mini", "gpt-5"], keyUrl: "https://platform.openai.com/api-keys", hint: "" },
  groq: { url: "https://api.groq.com/openai/v1", models: ["qwen/qwen3.8-27b", "openai/gpt-oss-120b", "openai/gpt-oss-20b"], keyUrl: "https://console.groq.com/keys", hint: "Same Groq key as speech-to-text works here. Free tier. Qwen keeps every sentence; gpt-oss-20b is faster but sometimes drops text." },
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
  $("zen-connect").hidden = p !== "zen";
  if (p !== "custom") $<HTMLInputElement>("llm-url").value = preset.url;
  const list = $<HTMLDataListElement>("llm-models");
  list.innerHTML = preset.models.map((m) => `<option value="${m}">`).join("");
  const modelInput = $<HTMLInputElement>("llm-model");
  const modelSelect = $<HTMLSelectElement>("llm-model-select");
  if (p === "zen") {
    // Zen: a real dropdown filled live from /v1/models; the text input stays as the source of truth.
    modelSelect.hidden = false;
    modelInput.placeholder = "model id (or pick above)";
    if ($<HTMLInputElement>("llm-key").value) void loadZenModels();
    else modelSelect.innerHTML = `<option value="">Connect OpenCode to list models</option>`;
  } else {
    modelSelect.hidden = true;
    if (!preset.models.includes(modelInput.value) && preset.models.length) modelInput.value = preset.models[0];
  }
  $("llm-hint").textContent = preset.hint;
  $<HTMLButtonElement>("llm-getkey").hidden = !preset.keyUrl;
  if (p === "ollama" && !$<HTMLInputElement>("llm-key").value) $<HTMLInputElement>("llm-key").value = "ollama";
}

async function loadZenModels() {
  const key = $<HTMLInputElement>("llm-key").value.trim();
  const select = $<HTMLSelectElement>("llm-model-select");
  const modelInput = $<HTMLInputElement>("llm-model");
  setStatus("zen-status", "Loading models…");
  try {
    const models = await invoke<ZenModel[]>("zen_models", { apiKey: key });
    const free = models.filter((m) => m.free);
    const paid = models.filter((m) => !m.free);
    const opt = (m: ZenModel) => `<option value="${m.id}">${m.id}  ·  ${m.wire}</option>`;
    select.innerHTML =
      (free.length ? `<optgroup label="Free">${free.map(opt).join("")}</optgroup>` : "") +
      `<optgroup label="Paid (billed to your OpenCode account)">${paid.map(opt).join("")}</optgroup>`;
    if (!models.some((m) => m.id === modelInput.value)) {
      modelInput.value = (free[0] ?? paid[0])?.id ?? "";
    }
    select.value = modelInput.value;
    setStatus("zen-status", `${models.length} models`, "ok");
  } catch (e) {
    setStatus("zen-status", String(e), "err");
  }
}

async function connectOpenCode() {
  const keyInput = $<HTMLInputElement>("llm-key");
  setStatus("zen-status", "Looking for OpenCode CLI credentials…");
  try {
    keyInput.value = await invoke<string>("import_opencode_key");
    setStatus("zen-status", "Key imported from the OpenCode CLI", "ok");
    await loadZenModels();
  } catch (e) {
    setStatus("zen-status", "Not logged in via the CLI. Opening opencode.ai/zen: paste the key below, then Refresh models.", "err");
    void invoke("open_url", { url: LLM_PRESETS.zen.keyUrl });
    keyInput.focus();
  }
}

function collect(): Config {
  return {
    hotkey: cfg.hotkey,
    stt: {
      base_url: $<HTMLInputElement>("stt-url").value.trim(),
      model: $<HTMLInputElement>("stt-model").value.trim(),
      api_key: $<HTMLInputElement>("stt-key").value.trim(),
      language: $<HTMLSelectElement>("stt-language").value,
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

let permTimer: number | undefined;

async function renderPermissions() {
  const perms = await invoke<Permission[]>("permission_status");
  const list = $("perm-list");
  list.innerHTML = perms
    .map(
      (p) => `<div class="perm ${p.granted ? "ok" : ""}" data-key="${p.key}">
        <span class="dot"></span>
        <span class="name">${p.label}</span>
        <span class="why">${p.granted ? "Granted" : p.detail}</span>
        <button data-grant="${p.key}">${p.can_prompt ? "Grant" : "Open Settings"}</button>
      </div>`,
    )
    .join("");
  list.querySelectorAll<HTMLButtonElement>("button[data-grant]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      btn.disabled = true;
      try {
        await invoke("request_permission", { key: btn.dataset.grant });
      } finally {
        btn.disabled = false;
        setTimeout(() => void renderPermissions(), 800);
      }
    });
  });
  const missing = perms.filter((p) => !p.granted);
  $("perm-note").textContent = missing.length
    ? "macOS only asks once, so use the buttons above: they open the right settings page. After switching one on, quit stfu from the menu bar and open it again — macOS only applies a new permission to a freshly started app (stfu restarts itself for Accessibility)."
    : "All set. Hold the hotkey anywhere and talk.";
  // Keep the panel honest while the user is away granting things in System Settings.
  if (permTimer) clearInterval(permTimer);
  if (missing.length) permTimer = window.setInterval(() => void renderPermissions(), 2500);
}

async function load() {
  cfg = await invoke<Config>("get_config");
  const languages = await invoke<Language[]>("languages");
  $<HTMLSelectElement>("stt-language").innerHTML = languages
    .map((l) => `<option value="${l.code}">${l.label}</option>`)
    .join("");
  $<HTMLSelectElement>("stt-language").value = cfg.stt.language ?? "";
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

  void renderPermissions();
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

$("perm-recheck").addEventListener("click", () => void renderPermissions());
$("stt-provider").addEventListener("change", (e) => applySttProvider((e.target as HTMLSelectElement).value));
$("zen-connect-btn").addEventListener("click", () => void connectOpenCode());
$("zen-refresh").addEventListener("click", () => void loadZenModels());
$("llm-model-select").addEventListener("change", (e) => { $<HTMLInputElement>("llm-model").value = (e.target as HTMLSelectElement).value; });
$("llm-key").addEventListener("change", () => { if ($<HTMLSelectElement>("llm-provider").value === "zen") void loadZenModels(); });
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

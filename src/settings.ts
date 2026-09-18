import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface SttConfig { base_url: string; api: string | null; model: string; api_key: string; language: string }
interface LlmConfig { enabled: boolean; base_url: string; model: string; api_key: string; timeout_secs: number; wire: string | null }
interface Profile { name: string; stt: SttConfig; llm: LlmConfig }
interface Config {
  hotkey: string[];
  active_profile: string;
  profiles: Profile[];
  launch_at_login: boolean;
  auto_update: boolean;
}
interface LocalStatus { ollama_up: boolean; ollama_models: string[]; whisper_up: boolean }

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

interface SttPreset { url: string; api: string | null; model: string; keyUrl: string; local?: boolean; setup?: string }

const STT_PRESETS: Record<string, SttPreset> = {
  groq: { url: "https://api.groq.com/openai/v1", api: null, model: "whisper-large-v3-turbo", keyUrl: "https://console.groq.com/keys" },
  openai: { url: "https://api.openai.com/v1", api: null, model: "whisper-1", keyUrl: "https://platform.openai.com/api-keys" },
  whispercpp: {
    url: "http://localhost:8080", api: "whispercpp", model: "", keyUrl: "", local: true,
    setup:
      "Runs entirely on this machine, and on Apple Silicon it uses the GPU. Install and start it once:" +
      '<code class="cmd">brew install whisper-cpp</code>' +
      '<code class="cmd">whisper-server -m ~/.whisper/ggml-base.en.bin --port 8080</code>' +
      "Small models are the fast ones: <b>tiny.en</b> or <b>base.en</b> keep dictation near-instant; <b>small.en</b> is more accurate and still usable. Download them from the whisper.cpp releases. No model name is needed here, because the server loads one at startup.",
  },
  speaches: {
    url: "http://localhost:8000/v1", api: null, model: "Systran/faster-whisper-small", keyUrl: "", local: true,
    setup:
      "A local server that speaks the OpenAI API, so it behaves exactly like the cloud option. Start it with Docker:" +
      '<code class="cmd">docker run --rm -p 8000:8000 ghcr.io/speaches-ai/speaches:latest</code>' +
      "Then use a small model such as <b>Systran/faster-whisper-small</b> or <b>-tiny</b> for speed.",
  },
  custom: { url: "", api: null, model: "", keyUrl: "" },
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
  ollama: {
    url: "http://localhost:11434/v1",
    models: ["llama3.2:3b", "qwen3.5:4b", "gemma3:4b", "phi4-mini", "mistral", "llama3.1:8b"],
    keyUrl: "https://ollama.com/download",
    hint: "Runs on this machine, no key and no network. Small models are the right choice here: cleaning up dictation is an easy task and speed matters more than depth.",
  },
  custom: { url: "", models: [], keyUrl: "", hint: "" },
};

function detect<T extends { url: string }>(presets: Record<string, T>, url: string): string {
  const hit = Object.entries(presets).find(([k, p]) => k !== "custom" && p.url && p.url === url.replace(/\/+$/, ""));
  return hit ? hit[0] : "custom";
}

let cfg: Config;
let editing = "";   // name of the profile the form is currently editing

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
  $("stt-setup").innerHTML = preset.setup ?? "";
  if (preset.local && !$<HTMLInputElement>("stt-key").value) $<HTMLInputElement>("stt-key").value = "local";
  void refreshLocalStatus();
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
  $("llm-setup").innerHTML =
    p === "ollama"
      ? "Install Ollama, then pull a small model once:" +
        '<code class="cmd">ollama pull llama3.2:3b</code>' +
        "Anything you have pulled shows up in the list below automatically."
      : "";
  void refreshLocalStatus();
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

function profileByName(name: string): Profile {
  return cfg.profiles.find((p) => p.name === name) ?? cfg.profiles[0];
}

/** The form always edits exactly one profile; this reads it back out. */
function collectProfile(): Profile {
  const previous = profileByName(editing);
  return {
    name: editing,
    stt: {
      base_url: $<HTMLInputElement>("stt-url").value.trim(),
      api: STT_PRESETS[$<HTMLSelectElement>("stt-provider").value]?.api ?? previous.stt.api,
      model: $<HTMLInputElement>("stt-model").value.trim(),
      api_key: $<HTMLInputElement>("stt-key").value.trim(),
      language: $<HTMLSelectElement>("stt-language").value,
    },
    llm: {
      enabled: $<HTMLInputElement>("llm-enabled").checked,
      base_url: $<HTMLInputElement>("llm-url").value.trim(),
      model: $<HTMLInputElement>("llm-model").value.trim(),
      api_key: $<HTMLInputElement>("llm-key").value.trim(),
      timeout_secs: previous.llm.timeout_secs,
      wire: previous.llm.wire,
    },
  };
}

function collect(): Config {
  const edited = collectProfile();
  return {
    hotkey: cfg.hotkey,
    active_profile: $<HTMLSelectElement>("profile-select").value,
    profiles: cfg.profiles.map((p) => (p.name === edited.name ? edited : p)),
    launch_at_login: cfg.launch_at_login,
    auto_update: $<HTMLInputElement>("auto-update").checked,
  };
}

function renderProfileList() {
  const sel = $<HTMLSelectElement>("profile-select");
  sel.innerHTML = cfg.profiles.map((p) => `<option value="${p.name}">${p.name}</option>`).join("");
  sel.value = editing;
  $<HTMLButtonElement>("profile-delete").disabled = cfg.profiles.length < 2;
}

/** Fill every provider field from one profile. */
function showProfile(name: string) {
  editing = name;
  const p = profileByName(name);

  const sttP = detect(STT_PRESETS, p.stt.base_url);
  $<HTMLSelectElement>("stt-provider").value = sttP;
  applySttProvider(sttP);
  $<HTMLInputElement>("stt-url").value = p.stt.base_url;
  $<HTMLInputElement>("stt-model").value = p.stt.model;
  $<HTMLInputElement>("stt-key").value = p.stt.api_key;
  $<HTMLSelectElement>("stt-language").value = p.stt.language ?? "en";

  const llmP = detect(LLM_PRESETS, p.llm.base_url);
  $<HTMLSelectElement>("llm-provider").value = llmP;
  $<HTMLInputElement>("llm-enabled").checked = p.llm.enabled;
  applyLlmProvider(llmP);
  $<HTMLInputElement>("llm-url").value = p.llm.base_url;
  $<HTMLInputElement>("llm-model").value = p.llm.model;
  $<HTMLInputElement>("llm-key").value = p.llm.api_key;

  setStatus("stt-status", "");
  setStatus("llm-status", "");
  void refreshLocalStatus();
}

/** Show whether the local servers this profile points at are actually running. */
async function refreshLocalStatus() {
  const sttP = $<HTMLSelectElement>("stt-provider").value;
  const llmP = $<HTMLSelectElement>("llm-provider").value;
  if (!STT_PRESETS[sttP]?.local && llmP !== "ollama") return;
  const status = await invoke<LocalStatus>("probe_local", {
    ollamaUrl: $<HTMLInputElement>("llm-url").value.trim() || "http://localhost:11434/v1",
    whisperUrl: $<HTMLInputElement>("stt-url").value.trim() || "http://localhost:8080",
  });
  if (STT_PRESETS[sttP]?.local) {
    setStatus(
      "stt-status",
      status.whisper_up ? "Local server is running." : "Nothing is answering on that address yet.",
      status.whisper_up ? "ok" : "err",
    );
  }
  if (llmP === "ollama") {
    if (status.ollama_models.length) {
      $<HTMLDataListElement>("llm-models").innerHTML = status.ollama_models
        .map((m) => `<option value="${m}">`)
        .join("");
      setStatus("llm-status", `Ollama is running, ${status.ollama_models.length} model(s) installed.`, "ok");
    } else {
      setStatus("llm-status", status.ollama_up ? "Ollama is running but has no models pulled yet." : "Ollama is not running.", "err");
    }
  }
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
  $("cfgpath").textContent = await invoke<string>("config_path");
  $("hotkey").textContent = cfg.hotkey.join(" + ");
  $<HTMLInputElement>("auto-update").checked = cfg.auto_update ?? true;
  $("app-version").textContent = `version ${await invoke<string>("app_version")}`;

  editing = cfg.active_profile || cfg.profiles[0].name;
  renderProfileList();
  showProfile(editing);
  void renderPermissions();

  const p = profileByName(editing);
  if (!p.stt.api_key) setStatus("stt-status", "No key yet. Click “Get a key”, paste it here, then Test.");
  if (!p.llm.api_key && p.llm.enabled) setStatus("llm-status", "No key yet. Click “Get a key”, paste it here, then Test.");
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
    renderProfileList();
    setStatus("save-status", `Saved. "${cfg.active_profile}" is active — hold the hotkey and talk.`, "ok");
  } catch (e) {
    setStatus("save-status", String(e), "err");
  }
}

$("profile-select").addEventListener("change", (e) => {
  const next = (e.target as HTMLSelectElement).value;
  // Keep edits to the profile being left, so switching never loses a pasted key.
  const edited = collectProfile();
  cfg.profiles = cfg.profiles.map((p) => (p.name === edited.name ? edited : p));
  cfg.active_profile = next;
  showProfile(next);
});

$("profile-new").addEventListener("click", () => {
  const name = prompt("Name for the new profile", "Local (offline)");
  if (!name) return;
  if (cfg.profiles.some((p) => p.name === name)) {
    setStatus("save-status", `A profile called ${name} already exists.`, "err");
    return;
  }
  // Start from a copy of the current one; the user then picks providers.
  const base = collectProfile();
  cfg.profiles = cfg.profiles.map((p) => (p.name === base.name ? base : p));
  cfg.profiles.push({ ...structuredClone(base), name });
  cfg.active_profile = name;
  editing = name;
  renderProfileList();
  showProfile(name);
  setStatus("save-status", `Created ${name}. Pick its providers below, then Save.`, "ok");
});

$("profile-delete").addEventListener("click", () => {
  if (cfg.profiles.length < 2) return;
  const name = editing;
  if (!confirm(`Delete the profile "${name}"?`)) return;
  cfg.profiles = cfg.profiles.filter((p) => p.name !== name);
  editing = cfg.profiles[0].name;
  cfg.active_profile = editing;
  renderProfileList();
  showProfile(editing);
});

$("perm-recheck").addEventListener("click", () => void renderPermissions());
$("check-updates").addEventListener("click", async () => {
  const btn = $<HTMLButtonElement>("check-updates");
  btn.disabled = true;
  setStatus("update-status", "Checking…");
  try {
    setStatus("update-status", await invoke<string>("check_for_updates"), "ok");
  } catch (e) {
    setStatus("update-status", String(e), "err");
  } finally {
    btn.disabled = false;
  }
});
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

// The tray can change the profile or language while this window sits open; re-read when it does.
listen("config-changed", () => void load());

load().catch((e) => setStatus("save-status", `Could not load config: ${e}`, "err"));

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface MeetingConfig { apps: string[] }
interface MeetingApp { id: string; label: string; enabled: boolean }
interface MeetingStatus {
  supported: boolean;
  running: boolean;
  model_name: string;
  model_mb: number;
  model_ready: boolean;
  downloading: boolean;
  screen_recording: boolean;
  folder: string;
  apps: MeetingApp[];
}

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

// The app list as last saved from this tab, so the main Save carries it instead of an old copy.
let savedApps: string[] | null = null;
let permTimer: number | undefined;

function status(id: string, text: string, kind: "" | "ok" | "err" = "") {
  const el = $(id);
  el.textContent = text;
  el.className = `status ${kind}`;
}

/** The meeting options to save with the rest of the config. */
export function meetingOptions(current: MeetingConfig | undefined): MeetingConfig | undefined {
  return savedApps ? { apps: savedApps } : current;
}

export async function showMeetings() {
  const s = await invoke<MeetingStatus>("meeting_status");
  $("mt-unsupported").hidden = s.supported;
  $("mt-body").hidden = !s.supported;
  if (!s.supported) return;

  $("mt-model").textContent = s.model_ready
    ? `${s.model_name} is downloaded.`
    : `${s.model_name}, ${s.model_mb} MB. Downloaded once, then everything runs offline.`;
  $("mt-download").hidden = s.model_ready;
  $<HTMLButtonElement>("mt-download").disabled = s.downloading;
  $("mt-delete").hidden = !s.model_ready;
  $<HTMLButtonElement>("mt-delete").disabled = s.running;

  const perm = $("mt-perm");
  perm.classList.toggle("ok", s.screen_recording);
  $("mt-perm-why").textContent = s.screen_recording
    ? "Granted"
    : "Lets stfu hear the call apps below. Nothing is recorded from the screen.";

  renderApps(s.apps);
  $("mt-folder").textContent = s.folder;

  // Keep the permission row honest while the user is away in System Settings.
  if (permTimer) clearInterval(permTimer);
  if (!s.screen_recording) {
    permTimer = window.setInterval(() => {
      if ($("tab-meetings").hidden) clearInterval(permTimer);
      else void showMeetings();
    }, 2500);
  }
}

function renderApps(apps: MeetingApp[]) {
  const list = $("mt-apps");
  list.replaceChildren(
    ...apps.map((a) => {
      const label = document.createElement("label");
      label.className = "toggle";
      const box = document.createElement("input");
      box.type = "checkbox";
      box.checked = a.enabled;
      box.dataset.id = a.id;
      box.addEventListener("change", () => void saveApps());
      label.append(box, a.label);
      return label;
    }),
  );
}

async function saveApps() {
  const apps = Array.from($("mt-apps").querySelectorAll<HTMLInputElement>("input[data-id]"))
    .filter((b) => b.checked)
    .map((b) => b.dataset.id!);
  try {
    await invoke("meeting_set_apps", { apps });
    savedApps = apps;
    status("mt-apps-status", "Saved. Applies from the next meeting.", "ok");
  } catch (e) {
    status("mt-apps-status", String(e), "err");
  }
}

async function download() {
  $<HTMLButtonElement>("mt-download").disabled = true;
  $("mt-meter").hidden = false;
  status("mt-model-status", "Downloading…");
  try {
    await invoke("meeting_download_model");
    status("mt-model-status", "Downloaded and verified.", "ok");
  } catch (e) {
    status("mt-model-status", String(e), "err");
  } finally {
    $("mt-meter").hidden = true;
    await showMeetings();
  }
}

async function remove() {
  try {
    await invoke("meeting_delete_model");
    status("mt-model-status", "Deleted.");
  } catch (e) {
    status("mt-model-status", String(e), "err");
  }
  await showMeetings();
}

listen<{ done: number; total: number }>("meeting-model-progress", (e) => {
  const { done, total } = e.payload;
  const pct = total ? Math.min(100, (done / total) * 100) : 0;
  ($("mt-meter").firstElementChild as HTMLElement).style.width = `${pct}%`;
  status("mt-model-status", `Downloading… ${Math.round(done / 1e6)} of ${Math.round(total / 1e6)} MB`);
});

$("mt-download").addEventListener("click", () => void download());
$("mt-delete").addEventListener("click", () => void remove());
$("mt-grant").addEventListener("click", async () => {
  await invoke("request_permission", { key: "screen_recording" }).catch(() => {});
  setTimeout(() => void showMeetings(), 800);
});
$("mt-folder-open").addEventListener("click", () => {
  invoke("meeting_open_folder").catch((e) => status("mt-apps-status", String(e), "err"));
});

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

let permTimer: number | undefined;
// App-list saves run one after another, each reading the checkboxes when its turn comes, so the
// last one to land is always the latest selection.
let saving: Promise<void> = Promise.resolve();

function status(id: string, text: string, kind: "" | "ok" | "err" = "") {
  const el = $(id);
  el.textContent = text;
  el.className = `status ${kind}`;
}

/** The meeting options to save with the rest of the config: the ticked apps once this tab has
 * shown them, otherwise the config as loaded. */
export function meetingOptions(current: MeetingConfig | undefined): MeetingConfig | undefined {
  return $("mt-apps").childElementCount ? { apps: tickedApps() } : current;
}

function tickedApps(): string[] {
  return Array.from($("mt-apps").querySelectorAll<HTMLInputElement>("input[data-id]"))
    .filter((b) => b.checked)
    .map((b) => b.dataset.id!);
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

  // Rendered once: re-rendering on a later refresh could undo a tick whose save is still queued.
  if (!$("mt-apps").childElementCount) renderApps(s.apps);
  $("mt-folder").textContent = s.folder;
  renderPermission(s.screen_recording);

  // Keep the permission row honest while the user is away in System Settings. Only that row is
  // refreshed, so a poll can never undo a checkbox the user has just changed.
  if (permTimer) clearInterval(permTimer);
  if (!s.screen_recording) {
    permTimer = window.setInterval(async () => {
      if ($("tab-meetings").hidden) return clearInterval(permTimer);
      const granted = (await invoke<MeetingStatus>("meeting_status")).screen_recording;
      renderPermission(granted);
      if (granted) clearInterval(permTimer);
    }, 2500);
  }
}

function renderPermission(granted: boolean) {
  $("mt-perm").classList.toggle("ok", granted);
  $("mt-perm-why").textContent = granted
    ? "Granted"
    : "Lets stfu hear the call apps below. Nothing is recorded from the screen.";
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
      box.addEventListener("change", () => {
        saving = saving.then(saveApps);
      });
      label.append(box, a.label);
      return label;
    }),
  );
}

async function saveApps() {
  try {
    await invoke("meeting_set_apps", { apps: tickedApps() });
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

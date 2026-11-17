import { invoke } from "@tauri-apps/api/core";

// The Dictionary tab: words to write a particular way and how the transcriber mishears them.
// Saved on its own (set_dictionary) as soon as a field changes; Settings' Save never touches it.

export interface DictEntry { word: string; sounds_like: string[]; fix_case: boolean }
interface Fix { from: string; to: string }
interface Preview { text: string; fixes: Fix[] }
interface Suggestion { text: string; count: number }

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

/** One editable row. `heard` is kept as typed ("Kuhn, Koon") and split when saving. */
interface Row { word: string; heard: string; fix_case: boolean }
let rows: Row[] = [];

/** createElement with a class and text; never innerHTML, since words can contain anything. */
function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls = "", text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
}

function status(text: string, kind: "" | "ok" | "err" = "") {
  const s = $("dict-status");
  s.textContent = text;
  s.className = `status ${kind}`;
}

function splitHeard(s: string): string[] {
  return s.split(",").map((x) => x.trim()).filter(Boolean);
}

/** What is typed right now, in the config's shape. */
export function dictionaryEntries(): DictEntry[] {
  return rows
    .filter((r) => r.word.trim())
    .map((r) => ({ word: r.word.trim(), sounds_like: splitHeard(r.heard), fix_case: r.fix_case }));
}

/** True while the cursor is in one of the rows, which must not be rebuilt under it. */
function editing(): boolean {
  return $("dict-list").contains(document.activeElement);
}

export function applyDictionary(list: DictEntry[] | undefined) {
  // Settings reloads the config whenever it is shown; the rows already save themselves, so a
  // reload while typing would only throw away the field in progress.
  if (editing()) return;
  rows = (list ?? []).map((e) => ({ word: e.word, heard: e.sounds_like.join(", "), fix_case: e.fix_case ?? true }));
  render();
  void preview();
}

let saveTimer: number | undefined;

/** Saves shortly after typing stops, so nothing typed is lost if the window closes. */
function saveSoon() {
  clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => void save(), 600);
}

async function save() {
  clearTimeout(saveTimer);
  try {
    const saved = await invoke<DictEntry[]>("set_dictionary", { entries: dictionaryEntries() });
    status(`Saved · ${saved.length} word${saved.length === 1 ? "" : "s"}. Used from your next dictation.`, "ok");
    // Show the tidied list (duplicates dropped, spacing fixed) once nobody is typing in it.
    if (!editing() && JSON.stringify(saved) !== JSON.stringify(dictionaryEntries())) applyDictionary(saved);
  } catch (e) {
    status(String(e), "err");
  }
  void preview();
}

async function suggest(row: Row, chips: HTMLElement, heardInput: HTMLInputElement) {
  chips.replaceChildren(el("span", "hint", "Looking through your history…"));
  try {
    const found = await invoke<Suggestion[]>("dictionary_suggest", { word: row.word, known: splitHeard(row.heard) });
    chips.replaceChildren();
    if (!found.length) {
      chips.append(el("span", "hint", "Nothing that sounds like it in your history yet."));
      return;
    }
    chips.append(el("span", "hint", "Heard in your history, click to add:"));
    for (const s of found) {
      const chip = el("button", "chip", `${s.text} · ${s.count}`);
      chip.title = `Seen ${s.count} time${s.count === 1 ? "" : "s"} in past transcripts`;
      chip.addEventListener("click", () => {
        row.heard = [...splitHeard(row.heard), s.text].join(", ");
        heardInput.value = row.heard;
        chip.remove();
        void save();
      });
      chips.append(chip);
    }
  } catch (e) {
    chips.replaceChildren(el("span", "hint", String(e)));
  }
}

function render() {
  const list = $("dict-list");
  list.replaceChildren();
  $("dict-empty").hidden = rows.length > 0;
  $("dict-head").hidden = rows.length === 0;
  rows.forEach((row, i) => {
    const box = el("div", "dict-row");

    const word = el("input");
    word.placeholder = "Koen";
    word.value = row.word;
    word.addEventListener("input", () => {
      row.word = word.value;
      saveSoon();
      void preview();
    });
    word.addEventListener("change", () => void save());

    const heard = el("input");
    heard.placeholder = "Kuhn, Koon, Cohen";
    heard.value = row.heard;
    heard.addEventListener("input", () => {
      row.heard = heard.value;
      saveSoon();
      void preview();
    });
    heard.addEventListener("change", () => void save());

    const caseBox = el("label", "toggle");
    caseBox.title = "Also fix the capitalisation of the word itself (github → GitHub). Switch off for a name that is also an everyday word, like Will or Mark.";
    const check = el("input");
    check.type = "checkbox";
    check.checked = row.fix_case;
    check.addEventListener("change", () => {
      row.fix_case = check.checked;
      void save();
    });
    caseBox.append(check, "Aa");

    const chips = el("div", "chips");
    const find = el("button", "", "Find");
    find.title = "Look through your dictation history for spellings that sound like this word";
    find.addEventListener("click", () => {
      if (!row.word.trim()) {
        word.focus();
        return;
      }
      if (/\s/.test(row.word.trim())) {
        chips.replaceChildren(el("span", "hint", "Find works on single words only."));
        return;
      }
      void suggest(row, chips, heard);
    });

    const remove = el("button", "", "✕");
    remove.title = "Remove this word";
    remove.addEventListener("click", () => {
      rows.splice(i, 1);
      render();
      void save();
    });

    box.append(word, heard, caseBox, find, remove, chips);
    list.append(box);
  });
}

async function preview() {
  const input = $<HTMLTextAreaElement>("dict-try").value;
  const out = $("dict-try-out");
  const fixes = $("dict-try-fixes");
  if (!input.trim()) {
    out.textContent = "";
    fixes.textContent = "";
    return;
  }
  try {
    const p = await invoke<Preview>("dictionary_preview", { entries: dictionaryEntries(), text: input });
    out.textContent = p.text;
    fixes.textContent = p.fixes.length ? p.fixes.map((f) => `${f.from} → ${f.to}`).join(" · ") : "No changes.";
  } catch (e) {
    fixes.textContent = String(e);
  }
}

$("dict-add").addEventListener("click", () => {
  rows.push({ word: "", heard: "", fix_case: true });
  render();
  document.querySelector<HTMLInputElement>("#dict-list .dict-row:last-child input")?.focus();
});
$("dict-try").addEventListener("input", () => void preview());

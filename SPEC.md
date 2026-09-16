# STFU — Voice Dictation Desktop App (Wispr Flow drop-in replacement)

Working title: **STFU** (the folder name). Rename freely.

Status: Draft v0.1, 2026-09-16
Target platforms: macOS (Apple Silicon + Intel), Windows 10/11 (x64, ARM64 nice-to-have)
Primary user: single developer / power user; team features explicitly out of scope for v1.

---

## 0. Project classification: PoC first, production track later

This is a personal tool, so v1 is treated as a **PoC-grade** app: local single-user, no accounts, no sync backend, minimal telemetry. The spec still lists the production concerns (code signing, notarisation, auto-update, crash reporting, secret handling, data retention) so nothing is silently skipped. Each is tagged **[PROD]** where it matters. Shortcuts taken in the PoC are flagged in §13.

---

## 1. What Wispr Flow does (feature inventory of the reference product)

Compiled from wisprflow.ai/features, the What's New changelog through 2026-09-15, and third-party reviews. Grouped by area. Every item has a priority for our clone:

- **P0** — must exist for the app to be usable as a daily replacement
- **P1** — needed for parity on the things that make Flow feel good
- **P2** — nice-to-have / later
- **OUT** — deliberately not building (team, mobile, meeting notetaker)

### 1.1 Capture & hotkey

| Feature | Description | Priority |
|---|---|---|
| Global push-to-talk hotkey | Hold a key (default: `Fn` on Mac, `Ctrl+Win` on Windows) anywhere in the OS; recording runs while held; release = transcribe + insert. | P0 |
| Hands-free / toggle mode | Double-tap the hotkey (or a second binding) to lock recording on; press again to stop. | P0 |
| Rebindable hotkey | Any modifier combination or single modifier; rejects combos the OS reserves. Separate binding for Command Mode. | P0 |
| Cancel shortcut | Default `Esc` while recording discards the take; rebindable. | P0 |
| Rebindable "send" key | Optional: after insert, press a key (or bind mouse button) to also press Enter in the target app (chat apps). | P1 |
| Mouse-button binding | Bind an extra mouse button as push-to-talk / toggle / send. | P2 |
| Long sessions | Up to 20 minutes continuous dictation, with a warning at 19 min. | P1 |
| Mid-dictation recovery | Audio auto-saved to disk; if transcription fails or the app crashes, offer inline retry with the saved audio. | P1 |
| Whisper mode | Works with whispered speech at close range; no toggle, just gain/VAD tuned for it. | P1 |
| Microphone selection & ranking | Pick a mic; ordered fallback list; auto-switch to external mic in clamshell mode; explicit errors for unplugged / in-use / permission-denied. | P0 (selection), P1 (ranking/fallback) |
| Virtual/routing audio devices | Works with Krisp, NVIDIA Broadcast, BlackHole, VB-Cable. | P1 |

### 1.2 Transcription & language

| Feature | Description | Priority |
|---|---|---|
| Fast STT | Sub-second perceived latency from key release to text appearing for short utterances. | P0 |
| 100+ languages | Auto-detect or pinned language. | P1 (auto-detect), P0 (pin one language) |
| Quick language switcher | Language picker in the floating bar; per-app language memory. | P2 |
| Mixed-language speech | Code-switching within one utterance. | P2 |
| Context-aware spelling of names | Reads on-screen text / clipboard / recent context to spell proper nouns correctly. | P1 |

### 1.3 AI formatting ("Refine")

| Feature | Description | Priority |
|---|---|---|
| Filler removal | Strips "um", "uh", "like", false starts. | P0 |
| Self-correction handling | "Send it Tuesday, no, Wednesday" -> "Send it Wednesday". | P0 |
| Auto punctuation & capitalisation | Sentence boundaries, question marks, commas. | P0 |
| Lists | Spoken enumerations become bulleted / numbered lists when the target app supports it. | P1 |
| Paragraph breaks | Inserts breaks on pauses / topic shifts. | P1 |
| Auto Cleanup level | None / Light / Medium / High; controls how aggressively the LLM rewrites. | P0 |
| Undo AI edit | Keep the raw transcript; one action swaps the inserted cleaned text for the verbatim version. | P1 |
| Consistent formatting model | Same behaviour across apps; regressions caught by benchmark set before shipping a prompt/model change. | P1 (eval set) |

### 1.4 Context awareness (per-app behaviour)

| Feature | Description | Priority |
|---|---|---|
| Active app detection | Knows which app / window / URL has focus. | P0 |
| App-specific styles | Formal in Mail/Outlook, casual in Slack/WhatsApp/Discord/Signal/Instagram/LinkedIn, terse in terminal. | P1 |
| Style tab | Default tone (formal / casual / enthusiastic / custom instruction), per-app overrides. English-only in Flow. | P1 |
| Text-field context | Reads the surrounding text in the focused field so the model can match tense, continue a sentence, avoid duplicate greetings. | P1 |
| Developer mode | Preserves `camelCase` / `snake_case`, recognises dev jargon (Supabase, Vercel, Cloudflare), keeps CLI commands intact, `@file` tagging for Cursor / Windsurf / Claude Code / Codex terminals. | P1 |
| Terminal visibility | Text insert works in Terminal / iTerm / Windows Terminal / Warp incl. Claude Code and Codex prompts. | P0 |
| Privacy-protected apps | Never capture or send context from password managers, banking apps, secure input fields (macOS Secure Input). Configurable blocklist. | P0 |

### 1.5 Personalisation

| Feature | Description | Priority |
|---|---|---|
| Personal dictionary | Custom words, names, acronyms; manual add/edit/delete/search; star important words; usage-based ranking; apostrophe-variant filtering. | P0 |
| Auto-learning from corrections | When the user edits inserted text within N seconds, diff it and propose dictionary entries. | P1 |
| Snippets | Voice trigger phrase -> expands to stored text (plain in v1; rich text bold/italic/links/lists later). | P0 (plain), P2 (rich) |
| Custom prompts / Transforms | Highlight text + shortcut -> rewrite via LLM. Built-ins: Polish, Prompt Engineer. User-defined transforms. Optional auto-apply after every dictation. | P1 |
| Command Mode | Separate hotkey; speak an instruction ("make that shorter", "turn into bullets", "translate to German") applied to the last dictation block or current selection. | P1 |

### 1.6 UI surfaces

| Feature | Description | Priority |
|---|---|---|
| Flow Bar | Small always-on-top floating pill showing recording state / waveform / processing; draggable to left or right screen edge; hides when idle. | P0 |
| Menu bar / tray icon | Status, quick toggles, open settings, quit. | P0 |
| Settings window | Tabs: General, Hotkeys, Microphone, Style, Dictionary, Snippets, Transforms, History, Insights, Privacy, Account/Plan (skip), About. | P0 |
| Dictation history | Last 14 days; text + raw transcript + audio playback; copy; retry; delete; search. | P1 |
| Scratchpad | Rich-text notepad opened with a shortcut (`Opt+S`); tabs, version history, image paste; synced in Flow. | P2 |
| Notifications | Native toasts for failures, tips, milestones; granular mute controls. | P1 |
| Insights | Words per minute, total words, dictionary/snippet replacements, app breakdown, streak heatmap, "voice profile". | P2 |
| Onboarding | Permission wizard (mic, accessibility, input monitoring), hotkey test, first-dictation tutorial. | P0 |
| UI localisation | EN, DE, ES, IT, PT in Flow. | P2 |

### 1.7 Data, privacy, account

| Feature | Description | Priority |
|---|---|---|
| Local data storage options | Normal / auto-delete after 24h / store nothing on device. | P1 |
| Cloud Sync toggle | Separate from privacy mode; dictionary/snippets/prompts sync. | OUT (v1 is local-only; export/import JSON instead) |
| Privacy mode | No transcripts retained server-side. | P0 as a *provider* setting (choose zero-retention STT/LLM endpoints; document it) |
| Account, SSO, billing, HIPAA BAA | | OUT |
| Bug reporting with logs | Zip logs + last audio (opt-in) to a folder. | P1 |

### 1.8 Team / enterprise / other products

| Feature | Priority |
|---|---|
| Shared dictionary, shared snippets, org/department word sharing, admin portal, leaderboards, usage dashboards, bulk invites, trials, seats | OUT |
| Notetaker (meeting recording, summaries, MCP, calendar) | OUT (separate project if ever) |
| iOS / Android keyboards, iPhone Shortcuts, Dynamic Island | OUT |
| Web demo | OUT |

---

## 2. Product goals and non-goals for v1

**Goals**

1. Hold hotkey, speak, release, clean text appears at the cursor in any app on macOS and Windows.
2. Latency that feels instant: median < 800 ms from key-up to first inserted character for a 10-second utterance (cloud STT), < 1.5 s end-to-end including LLM cleanup.
3. Personal dictionary + snippets + per-app style are good enough that the user stops noticing corrections.
4. Everything (audio, transcripts, dictionary) stays local except the audio/text sent to the chosen STT and LLM providers. Providers are pluggable, including fully local.

**Non-goals**

- Multi-user, sync, billing, mobile, meeting notes, browser extension.
- Pixel-perfect copy of Flow's UI.

---

## 3. Architecture

### 3.1 One codebase, two builds (recommended)

The user's instinct of "two desktop applications" is right at the *distribution* level: a `.dmg`/`.pkg` for macOS and an `.msi`/`.exe` for Windows. It is wrong at the *code* level. About 80 % of the app (audio pipeline, STT/LLM clients, formatting, dictionary, snippets, history, settings UI) is platform-neutral. Only the OS integration layer differs.

**Recommendation: Tauri 2 (Rust core + TypeScript/HTML UI), with a thin platform-adapter layer in Rust.** Tauri is "the Rust Electron". The comparison below covers the realistic candidates.

#### Framework comparison

Weighted for *this* app: an always-running background process whose hard parts are OS hooks (global key-up detection, accessibility text insertion, audio capture), not UI.

| Criterion | Tauri 2 (Rust + WebView) | Electron (Node + Chromium) | Two native apps (SwiftUI + WinUI 3 / C#) | Flutter desktop | Qt (C++/Rust bindings) |
|---|---|---|---|---|---|
| Installed size | ~10-15 MB | 150-250 MB | 5-20 MB each | ~30 MB | 40-80 MB |
| Idle RAM (background, window hidden) | 30-60 MB | 150-300 MB | 20-40 MB | 60-100 MB | 50-90 MB |
| Cold start | < 0.5 s | 1-3 s | < 0.3 s | ~1 s | ~0.5 s |
| Global hotkey incl. key-up and `Fn`/modifier-only | Rust: `CGEventTap` / `WH_KEYBOARD_LL` via `core-graphics`, `windows` crates. Direct FFI, no bridge. | Needs a native addon (N-API in C++/Rust, or `uiohook-napi`). The hard code is native anyway; Electron only adds a bridge. | Native, first-class. | Needs platform channel + native code per OS. | Needs native code per OS. |
| Accessibility text read/insert (AX / UIA) | Rust FFI via `objc2` / `accessibility` / `windows` crates. Some manual bindings work. | Native addon required; same effort as Tauri plus the N-API layer. | Native, first-class. | Platform channel + native. | Native per OS. |
| Audio capture | `cpal` (CoreAudio + WASAPI), one API. | Web Audio API in renderer (works, but the renderer must stay alive) or native addon. | AVAudioEngine / WASAPI separately. | `record` plugin. | QtMultimedia. |
| UI language | TypeScript (org default) | TypeScript | Swift + C# (two UIs) | Dart | C++ or Rust + QML |
| Code shared across OS | ~85 % (everything except adapters) | ~85 % | ~40 % (only if core is a shared Rust lib via FFI) | ~80 % | ~80 % |
| WebView consistency | WKWebView on Mac, WebView2 on Windows. Minor CSS differences; irrelevant for a settings UI. | Bundled Chromium, identical everywhere. | n/a | Own renderer, identical. | Own renderer. |
| Signing / store readiness | Tauri bundler + updater; notarisation supported. | electron-builder; mature. | Xcode / MSIX; most mature. | Supported. | Supported. |
| Ecosystem maturity for this use case | Good. Several Tauri dictation apps exist (open-source Whisper front-ends), proving the hooks work. | Excellent. Most mature. | Excellent. | Weak for accessibility APIs. | Adequate. |
| Fit for org defaults (TS, minimal deps) | Strong: TS UI, Rust core, no Node at runtime. | Strong on TS, weak on footprint. | Weak: two languages, neither TS. | Weak. | Weak. |
| Main risk | Rust learning curve for the adapter layer; occasional WebView2 quirks on Windows. | Footprint for an always-on app; still needs native addons. | Every feature built twice. | Poor accessibility API story. | Licensing (LGPL/commercial), C++. |

**Verdict.** Tauri wins on footprint and on putting the low-level code where it belongs (Rust), with a TypeScript UI. Electron would only win if we had no appetite for Rust, but the OS hooks force native code in every option, so that appetite is needed regardless. Two native apps is the fallback if Tauri blocks a P0 hook (see §3.4).

"Fastest" in the user's sense (startup, idle cost, hotkey-to-text latency) is dominated by the audio/STT/LLM pipeline, not the shell. The shell matters for footprint and start time, where Tauri is clearly ahead of Electron.

The UI is a small TS app (vanilla TS or a light framework; keep dependencies minimal per org policy). No Node runtime ships.

### 3.2 Component diagram

```
+--------------------------------------------------------------------+
|  Settings / History / Onboarding UI  (TypeScript, WebView)          |
+-----------------------------+--------------------------------------+
                              | Tauri IPC (commands + events)
+-----------------------------v--------------------------------------+
|  Core (Rust)                                                        |
|                                                                     |
|  Session FSM  ->  Audio Capture  ->  VAD/Gain  ->  STT Client       |
|       |                                              |              |
|       |          Context Collector (app, field text) |              |
|       |                    |                         v              |
|       |                    +--------> Formatter (LLM) -> Post-proc  |
|       |                                  (dictionary, snippets,     |
|       |                                   style, cleanup level)     |
|       v                                              |              |
|  Text Injector  <------------------------------------+              |
|                                                                     |
|  Storage (SQLite): history, dictionary, snippets, transforms, prefs |
+-------------------+------------------------------------------------+
                    | Platform adapter trait
       +------------+------------+
       v                         v
  macOS adapter             Windows adapter
  - CGEventTap hotkeys      - Low-level keyboard hook (WH_KEYBOARD_LL)
  - AXUIElement focus/text  - UI Automation (IUIAutomation) focus/text
  - CGEvent paste / AX set  - SendInput / clipboard paste
  - CoreAudio device list   - WASAPI device list
  - Secure Input detection  - Password field detection via UIA
  - NSStatusItem + panel    - Notify icon + layered window
```

### 3.3 Session state machine

```
Idle
  --hotkey down--> Recording (bar visible, waveform)
      --hotkey up (hold mode)--> Transcribing
      --double-tap--> RecordingLocked --hotkey--> Transcribing
      --Esc--> Idle (audio discarded, or kept if "save cancelled takes")
      --20 min--> Transcribing (auto-stop, warning at 19 min)
Transcribing
  --STT ok--> Formatting (skip if cleanup=None and no dictionary/snippet hits)
  --STT fail--> Failed (bar shows retry; audio kept)
Formatting
  --LLM ok--> Inserting
  --LLM fail/timeout--> Inserting (fallback: raw transcript with basic punctuation)
Inserting
  --done--> Idle (history row written; "Undo AI edit" available for 30 s)
```

Streaming variant (P1): while Recording, stream audio to STT and show partial transcript in the bar. On release, only the tail needs transcribing, cutting perceived latency.

### 3.4 Exit ramp

If Tauri's WebView or a crate blocks a P0 OS feature, the Rust core is kept and the shells are replaced with SwiftUI / WinUI 3 hosting the same Rust library via FFI (`uniffi` or C ABI). Keep the platform adapter trait clean from day one so this is a shell swap, not a rewrite.

---

## 4. Platform integration details

### 4.1 macOS

| Concern | Approach |
|---|---|
| Permissions | Microphone (AVFoundation), Accessibility (for AX text read/insert), Input Monitoring (for global key capture incl. `Fn`). Onboarding wizard deep-links to each System Settings pane and polls until granted. |
| Global hotkey | `CGEventTap` on `kCGHIDEventTap` for key down/up incl. modifier-only keys and `Fn` (flagsChanged). Must not swallow events unless the binding matches. |
| Focus & context | `AXUIElementCreateSystemWide` -> focused element; read `AXValue`, `AXSelectedTextRange`, role, window title, bundle id. For browsers, read URL via AX (Safari/Chrome expose `AXDocument`/URL attributes). |
| Text insertion | Preferred: `AXUIElementSetAttributeValue(kAXSelectedTextAttribute)` — instant, no clipboard pollution. Fallback: save clipboard -> set clipboard -> synthesize `Cmd+V` via `CGEvent` -> restore clipboard after ~200 ms. Terminal apps: fallback path only; Electron apps often need paste path. Per-app override table. |
| Secure Input | Detect `IsSecureEventInputEnabled()`; when true, disable dictation and show a lock icon in the bar. |
| Background app | `LSUIElement = true` (no Dock icon), `NSStatusItem` menu, floating `NSPanel` (non-activating) for the Flow Bar so focus never leaves the target app. |
| Login item | `SMAppService`. |
| Distribution [PROD] | Developer ID signing, hardened runtime, notarisation, Sparkle or Tauri updater with signed manifests. |

### 4.2 Windows

| Concern | Approach |
|---|---|
| Permissions | Microphone privacy toggle (Settings > Privacy). No accessibility permission model, but UIAccess / running non-elevated matters: hooks don't reach elevated windows. Document this. |
| Global hotkey | `SetWindowsHookEx(WH_KEYBOARD_LL)` for down/up of arbitrary keys including modifier-only. `RegisterHotKey` is insufficient (no key-up, no modifier-only). Raw Input as fallback. |
| Focus & context | `IUIAutomation` -> `GetFocusedElement`, `TextPattern` / `ValuePattern` for surrounding text; `GetForegroundWindow` -> process name; browser URL via UIA address bar or accessibility tree. |
| Text insertion | Preferred: UIA `ValuePattern.SetValue` / `TextPattern` where supported. Default: clipboard + `SendInput(Ctrl+V)` with clipboard restore. Terminals (Windows Terminal, conhost): `SendInput` unicode key events (`KEYEVENTF_UNICODE`) as a slower but universal fallback. |
| Password fields | Skip context capture when UIA `IsPassword` is true or the app is on the blocklist. |
| Background app | Notify icon (tray), layered topmost `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` window for the Flow Bar. |
| Login item | `HKCU\...\Run` or Task Scheduler entry; make it reliable (Flow had a bug here). |
| Distribution [PROD] | Authenticode (EV cert avoids SmartScreen warnings), MSI or NSIS via Tauri bundler, Tauri updater. |

### 4.3 Audio (both)

- Capture with `cpal` (CoreAudio / WASAPI) at 16 kHz mono PCM16, or native rate + resample.
- Device enumeration with hot-plug events; ranked preference list; "in use by another app" detection.
- Simple RMS/VAD gate for whisper mode gain, and to trim leading/trailing silence before upload.
- Ring-buffer the last 500 ms *before* key-down so clipped first syllables are recovered (Flow does this; it matters).
- Write each take to `takes/<uuid>.wav` (or Opus) for retry/history; purge per retention setting.

---

## 5. Speech-to-text providers

Pluggable trait `SttProvider { transcribe(audio, lang_hint, vocab_hints) -> Transcript }` plus optional `stream()`.

Note: OpenCode Zen (§6.1) has no speech-to-text, so STT is always a separate provider. Zero-cost path is local whisper.cpp.

| Provider | Mode | Notes |
|---|---|---|
| **Deepgram Nova-3** | cloud, streaming | Very low latency, keyword boosting (feed dictionary), 30+ languages. Good default. |
| **OpenAI gpt-4o-transcribe / whisper-1** | cloud, batch | Accurate, prompt field accepts vocabulary hints. |
| **ElevenLabs Scribe** | cloud, batch | Strong multilingual accuracy. |
| **Groq Whisper-large-v3-turbo** | cloud, batch | Cheap and fast. |
| **Local whisper.cpp / Parakeet (via `whisper-rs` or sherpa-onnx)** | local | Zero data leaves the machine; slower on Intel Macs; fine on Apple Silicon with Metal and on Windows with CUDA/DirectML. Required for the "fully private" mode. |

Requirements: pass dictionary words as vocabulary hints where the API supports it; return word timestamps if available (used for Undo-AI-edit alignment and future features); enforce a per-request timeout with fallback to the next provider in the list.

---

## 6. LLM formatting pipeline

### 6.1 Providers: bring your own model, OpenCode Zen first

The app ships with no bundled model and no bundled key. The user connects a provider in Settings > Style > Model. Providers are behind one trait `LlmProvider { complete(system, user, schema) -> FormattedResult }` with two wire formats implemented: **OpenAI chat-completions** and **Anthropic Messages**. That covers everything below.

| Provider | Wire format | Auth | Default? |
|---|---|---|---|
| **OpenCode Zen** (`https://opencode.ai/zen/v1`) | OpenAI-compatible (`/chat/completions`); some models also exposed via Anthropic-format endpoints | Zen API key from opencode.ai (sign in, copy key) | **Yes.** Free models available, so the app works at zero LLM cost out of the box. |
| Anthropic direct | Anthropic Messages | Anthropic API key | Optional. `claude-opus-5` per skill defaults; Haiku 4.5 selectable if latency measurements demand it. |
| OpenAI direct / any OpenAI-compatible URL (Groq, OpenRouter, Together) | OpenAI | provider key | Optional |
| Local: Ollama / llama.cpp server / LM Studio | OpenAI-compatible on localhost | none | Optional; required for fully-offline mode |

#### OpenCode Zen integration details

- **Model discovery.** Zen has no `/v1/models` endpoint yet (open issue anomalyco/opencode#2901). Ship a static model list in `models/opencode-zen.json` with `id`, `display`, `free: bool`, `wire: openai|anthropic`, `context`, and refresh it from a small JSON we host or from the docs page at build time. Users can type any model id manually.
- **Free tier.** The free set at time of writing (promotional, can disappear): Big Pickle, Union Alpha, MiMo-V2.5 Free, Ling 3.0 Flash Fin Free, Nemotron 3 Ultra Free, Nemotron 3.5 Lightning Free, Muse Spark 1.3 Contributor Free. Mark them with a "Free" badge, and put a small fast one (Nemotron 3.5 Lightning or Ling Flash) as the pre-selected default because dictation cleanup needs speed, not reasoning depth.
- **Model fallback chain.** Settings lets the user order models: e.g. `free-fast -> free-other -> paid`. On 429 / 5xx / timeout the next model is tried within the same 2.5 s budget. Free models will rate-limit under heavy use; the chain keeps dictation working.
- **Reasoning models.** Several free Zen models are reasoning models and emit thinking tokens, which kills latency. The provider sets `reasoning_effort: "low"` / disables thinking where the model supports it, and the default picker filters to non-reasoning models for the dictation role. Reasoning models remain available for Transforms and Command Mode, where a second of extra latency is acceptable.
- **Auth.** Zen key stored in the OS keychain like every other secret. First-run flow: "Get a free key at opencode.ai/zen" link, paste field, "Test" button that runs a one-sentence cleanup and shows the round-trip time.
- **Prompt caching.** Zen advertises cache pricing on models that support it. Keep the stable system prefix first so caching kicks in where available; do not depend on it for correctness.
- **What Zen does not give us.** Zen is text-only. There is no speech-to-text in Zen, so §5 still needs a separate STT provider. The zero-cost path is: local whisper.cpp for STT + Zen free model for cleanup. The lowest-latency path is: Deepgram/Groq for STT + Zen free fast model.
- **Structured output.** Not every Zen model honours `response_format: json_schema`. The provider requests JSON, then falls back to lenient parsing: strip code fences, take the first `{...}` block, and if that fails treat the whole response as the `text` field. Never fail a dictation because of malformed JSON.
- **Terms.** Free ids are promotional; the settings screen says so, and the app degrades gracefully to the next model when one is withdrawn.

#### Latency budget per provider role

| Role | Budget | Notes |
|---|---|---|
| Dictation cleanup | 2.5 s hard timeout, target p50 < 700 ms | Fast non-reasoning model, small prompt (~1.5k tokens with dictionary). |
| Command Mode / Transforms | 8 s | Reasoning models acceptable. |
| Dictionary suggestion extraction | background, 15 s | Can batch. |

Prompt caching where the provider supports it: the system prompt (rules + style + dictionary + snippets) is the stable prefix; only the transcript and app context vary per call.

### 6.2 What the model receives

```
system (cached):
  - role: dictation formatter, output ONLY the final text, no commentary
  - cleanup level rules (None/Light/Medium/High)
  - style for this app (formal/casual/terse/custom)
  - developer mode rules if terminal/IDE
  - dictionary: list of preferred spellings
  - snippets: trigger -> expansion table
  - self-correction, filler, list, paragraph rules
user (per call):
  - target app + window title + URL (if allowed)
  - preceding text in the field (last ~500 chars) and following text
  - raw transcript
```

Structured output: `{ text: string, dictionary_candidates: string[] }`. Use `output_config.format` so parsing never fails.

### 6.3 Guardrails

- Cleanup level **None** skips the LLM entirely; only dictionary regex replacement and snippet expansion run locally.
- Hard timeout (default 2.5 s). On timeout or error, insert the raw transcript with local punctuation heuristics and show a subtle "unpolished" badge in the bar.
- Never send context from blocklisted apps or secure fields; send the transcript only.
- Store both raw and formatted text in history to power "Undo AI edit" and the eval set.

### 6.4 Command Mode & Transforms

Same pipeline, different system prompt: input is `{instruction, target_text}` where target is the current selection or the last inserted block (tracked by range where AX/UIA allow, otherwise by re-selecting via Shift+Arrow synthesis, otherwise clipboard). Built-in transforms ship as editable prompt files.

### 6.5 Eval set (P1)

A folder of `{audio, raw_transcript, expected_output, app_context}` fixtures. Run before changing model or prompts. Start with 30 of the user's own dictations; grow from history with a "add to eval" button.

---

## 7. Data model (SQLite, local)

```
dictations(id, created_at, app_bundle, window_title, url, lang,
           raw_text, final_text, inserted_text, audio_path, stt_provider,
           llm_model, latency_ms_stt, latency_ms_llm, status, undo_available)
dictionary(id, word, pronunciation_hint, starred, use_count, source[manual|learned], created_at)
snippets(id, trigger, body, is_rich, use_count, created_at)
transforms(id, name, prompt, shortcut, auto_apply, builtin)
app_rules(app_id, style, language, insert_method, blocklisted, dev_mode)
settings(key, value_json)
```

Export/import all of the above as a single JSON file (replaces cloud sync).

---

## 8. Settings surface (complete list)

**General**: launch at login, show in menu bar/tray, Flow Bar position (left/right/hidden), theme, UI language, check for updates.
**Hotkeys**: push-to-talk, hands-free toggle (or double-tap), command mode, cancel, send-after-insert, scratchpad, mouse button bindings, conflict detection.
**Microphone**: device list with drag-to-rank, input level meter, test recording, whisper-mode gain, pre-roll buffer ms, silence trim.
**Style**: cleanup level, default tone, custom instruction, per-app overrides table, developer mode toggles, list/paragraph formatting toggles, LLM provider/model/key, local-only mode.
**Language**: pinned or auto; per-app language memory.
**Dictionary**: search, add, edit, star, delete, import CSV, learned-suggestions inbox.
**Snippets**: trigger, body, preview, test.
**Transforms**: list, edit prompt, shortcut, auto-apply.
**History**: list with search, play audio, copy raw/final, retry, delete, retention (14 d / 24 h / off).
**Insights** (P2): WPM, words, replacements, app breakdown, streak.
**Privacy**: app blocklist, secure-input behaviour, context sharing toggle (send surrounding text yes/no), audio retention, STT/LLM provider retention notes, "delete everything".
**Advanced**: insertion method per app, STT provider order, timeouts, logs folder, export/import JSON, bug report bundle.

---

## 9. Non-functional requirements

| Area | Target |
|---|---|
| Latency | p50 < 800 ms key-up to text (cloud STT, cleanup Light); p95 < 2 s. Streaming STT P1 to hit p50 < 400 ms. |
| Idle footprint | < 80 MB RAM, ~0 % CPU when idle; hook thread only. |
| Reliability | No dropped first syllable (pre-roll), no lost audio on failure (disk-backed), no clipboard corruption (restore verified). |
| Security | API keys in OS keychain (Keychain / Credential Manager), never in plaintext config. No telemetry in PoC. [PROD] signed binaries, updater signatures, dependency audit (`cargo audit`, pnpm audit), 3-day `minimumReleaseAge` for npm packages per org policy. |
| Accessibility of the app itself | Settings UI keyboard-navigable; bar has reduced-motion option. |
| Offline | Local STT + local LLM mode works with network off; cloud mode fails fast with a clear message. |
| Logging | Rotating local logs, no transcript content at default level. |

---

## 10. Milestones

**M0 — Spike (1 week)**: Tauri skeleton; global hold-hotkey on both OSes; record WAV; send to one STT; clean up via OpenCode Zen free model; paste result. Measure per-model latency across the free Zen set. Proves the hard parts (hotkey key-up, `Fn`, insertion into Slack, VS Code, Terminal, Chrome).

**M1 — Daily-usable (P0)**: session FSM, Flow Bar, tray, mic selection, cleanup levels with LLM, dictionary, snippets, app detection + blocklist, secure input, onboarding permissions wizard, history (text only), settings persistence, keychain secrets.

**M2 — Parity on feel (P1)**: streaming STT partials, pre-roll buffer, hands-free double-tap, per-app styles + text-field context, developer mode, Command Mode, Transforms, Undo AI edit, audio history + retry, mic ranking/fallback, virtual devices, notifications, learned dictionary suggestions, eval set, export/import.

**M3 — Polish (P2)**: language switcher, insights, scratchpad, rich snippets, UI localisation, mouse bindings, auto-update, signing/notarisation, installers.

---

## 11. Repository layout (proposed)

```
stfu/
  SPEC.md
  apps/desktop/            Tauri app (src-tauri = Rust core + adapters, src = TS UI)
    src-tauri/src/
      core/                fsm, audio, stt/, llm/, formatter, storage, injector
      platform/            mod.rs (trait), macos/, windows/
    src/                   settings UI (TS)
  evals/                   fixtures + runner
  docs/                    ADRs, permission walkthroughs with screenshots
```

Tooling: pnpm (`--ignore-scripts`), Rust stable, `cargo-deny`, GitHub Actions matrix (macos-14, windows-latest) producing unsigned artefacts in PoC.

---

## 12. Open questions for the user

1. **STT default**: cloud (Deepgram, fastest) vs local-first (whisper.cpp, private)? Recommend cloud default with local as a toggle.
2. **LLM default model**: spec defaults to OpenCode Zen with a free fast model; Anthropic `claude-opus-5` or others are opt-in. Confirm which free Zen model wins on latency once M0 measures it.
3. **Send surrounding text to the LLM by default?** Big accuracy win, mild privacy cost. Recommend on, with the blocklist.
4. **Windows ARM64** build: needed?
5. **Hotkey defaults**: `Fn` on Mac is what Flow uses but conflicts with macOS's own dictation/emoji binding; user must disable that in System Settings. Acceptable?

---

## 13. PoC shortcuts taken (flagged)

- No code signing / notarisation: Gatekeeper and SmartScreen warnings on first launch.
- No auto-update; manual reinstall.
- No crash reporting or telemetry.
- Single provider key per service, no key rotation UI.
- Depends on OpenCode Zen free models that are promotional and may be withdrawn; paid fallback requires the user to add a key.
- Settings UI is functional, not designed.
- Team, sync, mobile, meeting features skipped entirely (see §1.8).

---

## Sources

- https://wisprflow.ai/features
- https://wisprflow.ai/whats-new (release notes through 2026-09-15)
- https://docs.wisprflow.ai
- https://tldv.io/blog/wisprflow/
- https://willowvoice.com/blog/wispr-flow-review-voice-dictation
- https://spokenly.app/blog/wispr-flow-review
- https://sidsaladi.substack.com/p/wispr-flow-101-the-complete-guide
- https://opencode.ai/docs/zen/
- https://github.com/anomalyco/opencode/issues/2901 (no /v1/models endpoint on Zen yet)

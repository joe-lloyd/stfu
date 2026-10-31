# stfu

Hold a key, talk, release. Your speech is transcribed, cleaned up by an LLM (fillers removed, punctuation fixed, lists turned into bullets), and pasted wherever your cursor is. A small floating pill shows a live waveform while you speak.

MVP scope. See `SPEC.md` for the full feature spec this grows towards.

## How it works

```
hold hotkey -> record mic -> release -> speech-to-text -> LLM cleanup -> Cmd/Ctrl+V into the focused app
```

- Rust core (Tauri 2): global hotkey via a native CGEventTap on macOS and `rdev` on Windows, audio via `cpal`, paste via clipboard + `enigo`.
- Tiny TypeScript webview for the waveform pill. No settings UI yet: edit `config.json`.
- Speech-to-text: any OpenAI-compatible `/audio/transcriptions` endpoint. Default is Groq `whisper-large-v3-turbo` (fast, cheap).
- Cleanup: any OpenAI-compatible `/chat/completions` endpoint. Default is Groq `qwen/qwen3.8-27b` on the free tier, so one Groq key covers both roles. OpenCode Zen, OpenAI and local Ollama are presets too.

## Install (one command)

Grab the latest [release](https://github.com/joe-lloyd/stfu/releases). Builds are unsigned, so the installers also clear the download quarantine that would otherwise make Gatekeeper or SmartScreen refuse to open them.

macOS or Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/joe-lloyd/stfu/main/scripts/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/joe-lloyd/stfu/main/scripts/install.ps1 | iex
```

Pin a version with `STFU_VERSION=v0.1.0` (or `$env:STFU_VERSION`). Windows hotkey is **Ctrl+Win**; macOS is **Fn**.

## Launch at login

On by default: the app registers itself to start with your session (a LaunchAgent on macOS, the `Run` registry key on Windows) and re-applies that registration on every start, so it survives updates moving the bundle. Toggle it from the tray menu ("Launch at login") or set `launch_at_login` in the config file.

## Auto-update

The app checks GitHub Releases about 20 seconds after launch and every six hours after that, verifies the download against the minisign public key in `tauri.conf.json`, installs, and restarts itself. Settings has an **Updates** section showing the running version, a **Check for updates now** button, and a toggle for the background checks (on by default, stored as `auto_update`). The tray menu has the same manual check. Releases are signed with the same `stfu Dev Signing` certificate every time, so on macOS the Accessibility, Input Monitoring and Microphone grants survive updates.

Secrets that make this work live in the GitHub repo (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). The originals are on Joe's Mac in `~/.stfu-signing/` and `~/.tauri/`. **Back those up**: losing the updater private key means existing installs can never update again, and losing the certificate means one more round of permission prompts.

## Releasing

`main` only accepts pull requests. To release, run this from a clean, up-to-date `main`:

```sh
scripts/release.sh 0.10.0
```

It bumps the version in `package.json`, `tauri.conf.json`, `Cargo.toml` and `Cargo.lock` on a `release/v0.10.0` branch, opens and merges a `release: v0.10.0` pull request, then tags the merged commit and pushes the tag. It needs an authenticated `gh`. The tag starts `release.yml`, which builds macOS (universal), Windows (x64) and Linux (x64) and publishes a release with the install commands in its notes.

`check.yml` type-checks all three platforms on every pull request and every push to `main`. Both workflows run on GitHub-hosted runners; change `runs-on` to a self-hosted label to move a job onto a lab machine.

## Build from source

Prerequisites: Rust (`rustup`), Node 20+, pnpm. On macOS, Xcode command line tools. On Windows, the WebView2 runtime (preinstalled on Windows 11) and the MSVC build tools.

```sh
pnpm install --ignore-scripts
pnpm tauri dev          # run
pnpm tauri build        # produce .app/.dmg (macOS) or .msi/.exe (Windows)
```

### Dictation languages

The default is **English**. Switch from the tray menu's **Language** submenu (instant, no Settings window) or the dropdown in Settings. Twenty languages are listed, including Dutch, plus Auto-detect; any other ISO-639-1 code works as `stt.language` in the config.

The chosen language does two things. It is passed to the transcriber as a hint, and it loads that language's own clean-up rules into the system prompt: which words are fillers, how days and months are capitalised, and a worked example in that language. Dutch, for instance, lower-cases `maandag`, capitalises both letters of `IJmuiden`, and keeps English loanwords like "de build" and "deployen" untranslated.

Selecting a language never causes translation. With Dutch selected, an English sentence still comes out in English. That is deliberate: telling a model to "write the output in Dutch" makes it translate the occasional English dictation, which was measured and rejected.

## Profiles: cloud for speed, local for privacy

A profile is a complete set of providers. Two ship by default:

| Profile | Speech to text | Clean-up | Network |
|---|---|---|---|
| Cloud (free) | Groq `whisper-large-v3-turbo` | Groq `qwen/qwen3.8-27b` | yes |
| Local (offline) | whisper.cpp server on `:8080` | Ollama `llama3.2:3b` | none |

Switch from the tray's **Profile** submenu or the dropdown in Settings; the tray and the window stay in step. Add your own with **New…**, which copies the profile you are on. An existing config from before profiles is folded into "Cloud (free)" automatically with its keys intact.

### Running fully offline

Nothing leaves the machine on the local profile, and no API key is needed: any `localhost` endpoint is treated as keyless.

**Speech to text.** whisper.cpp is the fastest option on Apple Silicon because it uses the GPU:

```sh
brew install whisper-cpp
curl -L -o ~/.whisper/ggml-base.en.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
whisper-server -m ~/.whisper/ggml-base.en.bin --port 8080
```

Small models are the fast ones. Measured here with `tiny.en`: a ten-word sentence transcribed in **0.4 s**. `base.en` is the sensible default, `small.en` is more accurate and still usable. The server loads one model at startup, so no model name is needed in Settings. [Speaches](https://speaches.ai) is the alternative if you prefer Docker and an OpenAI-compatible API.

**Clean-up.** Ollama, no key and no network:

```sh
ollama pull llama3.2:3b
```

Cleaning up dictation is an easy task, so a small model is the right call: `llama3.2:3b`, `qwen3.5:4b`, `gemma3:4b` and `phi4-mini` all handle it and keep the round trip short. Settings lists whatever you have pulled automatically and says whether Ollama is actually running.

### OpenCode as the clean-up provider

Pick "OpenCode" in Settings and press **Connect OpenCode**. If you have run `opencode auth login` and signed in to OpenCode Zen, the key is imported from the CLI's `auth.json`; otherwise the Zen page opens for you to paste one. The model dropdown is then filled live from Zen's model list, Free models first. Every Zen model is routed to its native endpoint automatically (chat completions, Responses, Anthropic Messages or Gemini), so Claude, GPT, Gemini, DeepSeek, Kimi, GLM and the rest all work through the one account. Zen has no speech-to-text, so section 1 still needs a Groq or OpenAI key. Set `llm.wire` in the config to force a wire format for a custom endpoint.

### Keys

On first launch (or whenever a key is missing) the **Settings** window opens. Pick a provider, click "Get a key" to open the provider's key page, paste the key, hit **Test**, then **Save**. Settings is also in the menu bar / tray menu.

Alternatively, export keys as environment variables:

```sh
export STFU_STT_API_KEY=...
export STFU_LLM_API_KEY=...
```

or put them in the config file created on first launch:

- macOS: `~/Library/Application Support/stfu/config.json`
- Windows: `%APPDATA%\stfu\config.json`

```json
{
  "hotkey": ["Function"],
  "stt": { "base_url": "https://api.groq.com/openai/v1", "model": "whisper-large-v3-turbo", "api_key": "", "language": "" },
  "llm": { "enabled": true, "base_url": "https://api.groq.com/openai/v1", "model": "qwen/qwen3.8-27b", "api_key": "", "timeout_secs": 8 }
}
```

Hotkey names: `Function`, `ControlLeft`, `ControlRight`, `MetaLeft`, `MetaRight`, `Alt`, `AltGr`, `ShiftLeft`, `ShiftRight`, `CapsLock`. Multiple names means all must be held. On macOS the listener reads modifier flags, so left and right variants are treated the same and only modifier keys can be used. Windows default is `["ControlLeft", "MetaLeft"]` (Ctrl+Win).

Set `llm.enabled` to `false` to paste the raw transcript with no cleanup. Any OpenAI-compatible provider works for either role, including a local Ollama or LM Studio server (`http://localhost:11434/v1`).

### macOS permissions

The app needs three, and **Settings shows a live panel with all three and a button each**. It opens by itself whenever something is missing.

| Permission | What it does | If missing |
|---|---|---|
| Microphone | Hears you | macOS hands the app silence, so nothing is transcribed |
| Input Monitoring | Notices the hotkey being held | The hotkey does nothing at all, with no error |
| Accessibility | Pastes the text into the app you are in | Text is transcribed but never appears |

macOS only ever prompts once per permission. If a prompt is missed there is no second chance, which is why the panel exists: its buttons open the exact System Settings page. Accessibility and Input Monitoring only apply to a freshly started process, so the app restarts itself as soon as either is granted.

Run the built `stfu.app` (from `pnpm tauri build --debug --bundles app`, or `open src-tauri/target/debug/bundle/macos/stfu.app`) rather than `pnpm tauri dev` when testing permissions: in dev mode macOS attributes them to the terminal that launched the process, and a missing microphone grant shows up as silence rather than an error.

Builds are signed with a local self-signed identity named `stfu Dev Signing` so grants survive rebuilds. Create one once with Keychain Access (Certificate Assistant > Create a Certificate, type Code Signing) or remove `signingIdentity` from `tauri.conf.json` to fall back to ad-hoc signing, in which case you must re-grant after every build. The `Entitlements.plist` audio-input entry is required under the hardened runtime; without it the microphone is denied silently.

Logs: `~/Library/Application Support/stfu/stfu.log`. Last recording: `last.wav` in the same folder.

The default macOS hotkey is `Fn`. macOS binds `Fn` to dictation or emoji by default: turn that off in System Settings > Keyboard > "Press fn key to" > Do Nothing, otherwise both fire.

### Windows

Default hotkey is Ctrl+Win (`["ControlLeft", "MetaLeft"]`). There are no permission dialogs, but two things catch people out:

- **The tray icon is hidden by default.** Click the `^` arrow next to the clock and drag the waveform icon onto the taskbar.
- **Microphone privacy.** If nothing is recorded, allow desktop apps to use the microphone under Settings > Privacy & security > Microphone. Settings shows a Microphone row that goes red when the device cannot be opened. The keyboard hook cannot see elevated (admin) windows, so dictation into an admin PowerShell will not work unless stfu also runs elevated.

## History

Every dictation is written to a local file, one per day, so nothing is ever lost and you can see what each stage did:

```
~/Library/Application Support/stfu/history/2026-09-28.jsonl          (Windows: %APPDATA%\stfu\history, Linux: ~/.config/stfu/history)
~/Library/Application Support/stfu/history/audio/2026-09-28/<id>.wav   (or <id>.flac once compressed)
```

Each line is one JSON object holding the whole take: when it happened and which app had focus, the profile and language, the audio (length, how long the key was held, the WAV file), the speech-to-text model, timing, raw transcript and full server reply, the clean-up model with the exact message it was sent, its raw reply, the cleaned text and any error or fallback, and finally what was pasted and whether pasting worked. Failed and empty takes are recorded too, with their error. Taps under 300 ms are not.

**Limits.** Transcripts are kept for good: a transcript costs a couple of KB, while audio is nearly all of the space, so the limits only ever remove recordings. By default recordings are capped at 500 MB and kept for 30 days; past either, a day's recordings go and its transcripts and results stay. All three are editable in the History tab (0 means no limit). If you do want old transcripts gone, set "Keep transcripts (days)"; days past it are removed entirely. A config saved before 0.9.1 with `keep_days` keeps that number, now as the age limit for recordings.

**Compression.** At 80% of the recordings cap, recordings from before today are compressed by themselves to FLAC: lossless, so they play back sample for sample as recorded, at about half the size (measured on real takes: zip saves ~22%, FLAC ~46-50%). Nothing is deleted and nothing asks. Each file is decoded again and compared with the original before its WAV is removed. **Compress now** in the History tab does the same at any time. The encoder is built in (`src-tauri/src/flac.rs`, no dependency) and writes standard `.flac` files any player opens. Only when compression is no longer enough does the warning below appear.

Over the cap, the oldest days' recordings go first and their transcripts and results stay. The History tab shows how much is used, warns at 80% of the cap, and says exactly what a cleanup would delete. With "Ask me before deleting anything" on (the default) nothing is deleted until you press Clean up now and confirm, and if a cleanup is waiting when stfu starts, Settings opens on the History tab to ask. Turn it off to let cleanup run by itself after each dictation. Today's history is never removed by a cleanup.

Open **Settings > History** to browse by day, search, play the audio, copy the output, the raw transcript or the full record, open the folder, delete a day's recordings (its transcripts stay), or delete a day entirely. Recording history and keeping audio can each be switched off there. Audio costs about 32 KB per second of speech.

The history never leaves the machine. The folder and files are readable by your user only (0700/0600 on macOS and Linux). Because the files are plain JSON Lines they also work with other tools, e.g. `jq -r .output.text ~/Library/Application\ Support/stfu/history/*.jsonl`.

## Meeting transcripts

macOS 13 or newer. Choose **Start meeting transcript** in the menu bar when a call begins and **Stop meeting transcript** when it ends. stfu transcribes both sides of the call on this machine and writes them to a plain text file, ready to paste into another tool:

```
~/Library/Application Support/stfu/meetings/2026-10-31_14-05-33.txt
```

```
[00:12:30] You: I can take the migration plan, I'll have it by Friday.
[00:12:41] Others: Great, then Sarah does the payment crash.
```

- **Local only.** A small English model, NVIDIA Parakeet TDT-CTC 110M (135 MB), runs on the CPU through [transcribe.cpp](https://github.com/handy-computer/transcribe.cpp). Download it once from Settings > Meetings; the download is checked against its SHA-256 before it is used. Nothing is uploaded and no API key is needed.
- **No audio is kept.** Audio is held in memory for about 30 seconds, transcribed, and dropped. Each chunk's text is appended as soon as it is ready, so a crash loses at most the chunk in progress.
- **Only call apps are heard.** "You" is the microphone. "Others" comes from ScreenCaptureKit, filtered to the apps ticked in Settings > Meetings: Zoom, Teams, Slack, FaceTime, Webex, Discord and the common browsers. Spotify, Music and anything else are never captured. A browser is heard as a whole, so a YouTube tab in the same browser as a Google Meet call is heard too; pause it, or take calls in another browser and untick this one.
- **Screen Recording permission.** macOS requires it to hear other apps, although nothing is recorded from the screen. Settings > Meetings has a button for it. A new grant only applies after a restart, which macOS offers as Quit & Reopen.
- **Wear headphones.** On speakers the microphone hears the call, and your music, again.
- **Nobody else is notified.** Unlike Zoom's or Teams' own recording, the other people see nothing. Tell them, and check whether their jurisdiction requires consent.
- **Speed.** Chunks end at the quietest moment of their last six seconds, so words are rarely cut in two. Transcription uses half the CPU cores. Intel Macs run about 3.7x faster than real time, because transcribe.cpp builds without AVX2 there so the app runs on every Intel Mac; Apple silicon is faster.

**Copy last meeting transcript** in the menu bar puts the newest one on the clipboard. Windows and Linux are not supported yet: both need their own per-app audio capture.

## Behaviour details

- Taps shorter than 300 ms are ignored.
- The clipboard is restored to its previous text ~250 ms after pasting, unless no text field has focus: then the dictation stays on the clipboard so it is not lost, and the pill says "Copied to clipboard". Settings > "Leave the dictation on the clipboard" also offers Always and Never. macOS only; the check uses Accessibility. Chrome and Electron apps (Slack, VS Code, Teams, ...) do not report their focused element, so there the old clipboard comes back as before.
- The last dictation is always one click away: **Copy last dictation** in the menu bar, or the Last dictation card at the top of Settings.
- If the LLM call fails or times out, the raw transcript is pasted so you never lose a dictation.
- Errors show in the pill for 2.5 s and in the log (`RUST_LOG=info`).

## If it is not working

Open Settings from the tray icon. The Permissions panel at the top is the answer to almost every "it does nothing" report; a red row names the problem and its button opens the right page.

Beyond that:

| Symptom | Cause |
|---|---|
| Hotkey does nothing, no pill appears | Input Monitoring missing, or another dictation app (Wispr Flow, macOS dictation on Fn) is grabbing the same key |
| Pill appears but the wave is flat | Microphone missing or the wrong input device is selected in the OS |
| Text is transcribed but nothing is pasted | Accessibility missing, or the target app refuses synthetic paste |
| "no speech-to-text API key" | No key saved yet; add one in Settings and press Test |

The log is at `~/Library/Application Support/stfu/stfu.log` on macOS and `%APPDATA%\stfu\stfu.log` on Windows. It records each permission's state at startup.

## Known limitations (MVP)

- API keys live in a plain JSON file or env vars, not the OS keychain.
- Settings covers providers and keys only. No dictionary, snippets, history or per-app styles yet. All in `SPEC.md`.
- Paste goes through the clipboard; apps that block paste (some password fields, some terminals) will not receive text.
- OpenCode Zen free models are promotional and may be withdrawn; switch `llm.model` if one stops responding. Some Zen models are served on `/responses` or `/messages` instead of `/chat/completions`; pick a model listed under chat completions.
- Unsigned builds: Gatekeeper and SmartScreen will warn on first launch.

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

Push a tag and GitHub Actions builds macOS (universal), Windows (x64) and Linux (x64) and publishes a release with the install commands in its notes:

```sh
git tag v0.2.0 && git push origin v0.2.0
```

`check.yml` type-checks all three platforms on every push to `main`. Both workflows run on GitHub-hosted runners; change `runs-on` to a self-hosted label to move a job onto a lab machine.

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

## Behaviour details

- Taps shorter than 300 ms are ignored.
- The clipboard is restored to its previous text ~250 ms after pasting.
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

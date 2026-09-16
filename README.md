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
- Cleanup: any OpenAI-compatible `/chat/completions` endpoint. Default is OpenCode Zen with the free `big-pickle` model.

## Setup

Prerequisites: Rust (`rustup`), Node 20+, pnpm. On macOS, Xcode command line tools. On Windows, the WebView2 runtime (preinstalled on Windows 11) and the MSVC build tools.

```sh
pnpm install --ignore-scripts
pnpm tauri dev          # run
pnpm tauri build        # produce .app/.dmg (macOS) or .msi/.exe (Windows)
```

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
  "llm": { "enabled": true, "base_url": "https://opencode.ai/zen/v1", "model": "big-pickle", "api_key": "", "timeout_secs": 8 }
}
```

Hotkey names: `Function`, `ControlLeft`, `ControlRight`, `MetaLeft`, `MetaRight`, `Alt`, `AltGr`, `ShiftLeft`, `ShiftRight`, `CapsLock`. Multiple names means all must be held. On macOS the listener reads modifier flags, so left and right variants are treated the same and only modifier keys can be used. Windows default is `["ControlLeft", "MetaLeft"]` (Ctrl+Win).

Set `llm.enabled` to `false` to paste the raw transcript with no cleanup. Any OpenAI-compatible provider works for either role, including a local Ollama or LM Studio server (`http://localhost:11434/v1`).

### macOS permissions

The app needs three permissions. macOS attributes them to the *process that asks*, so in `pnpm tauri dev` that is your terminal app (Terminal, iTerm, T3 Code, ...), and in the built `.app` it is stfu itself.

1. **Microphone**: prompted automatically on first recording.
2. **Accessibility**: System Settings > Privacy & Security > Accessibility. Needed to observe key-up of the hotkey and to send the paste keystroke.
3. **Input Monitoring**: System Settings > Privacy & Security > Input Monitoring. Needed for the global key listener.

If you see `global key listener failed` in the log, one of the last two is missing. Restart the app after granting.

The default macOS hotkey is `Fn`. macOS binds `Fn` to dictation or emoji by default: turn that off in System Settings > Keyboard > "Press fn key to" > Do Nothing, otherwise both fire.

### Windows

No permission dialogs. The keyboard hook cannot see elevated (admin) windows, so dictation into an admin PowerShell will not work unless stfu also runs elevated.

## Behaviour details

- Taps shorter than 300 ms are ignored.
- The clipboard is restored to its previous text ~250 ms after pasting.
- If the LLM call fails or times out, the raw transcript is pasted so you never lose a dictation.
- Errors show in the pill for 2.5 s and in the log (`RUST_LOG=info`).

## Known limitations (MVP)

- API keys live in a plain JSON file or env vars, not the OS keychain.
- Settings covers providers and keys only. No dictionary, snippets, history or per-app styles yet. All in `SPEC.md`.
- Paste goes through the clipboard; apps that block paste (some password fields, some terminals) will not receive text.
- OpenCode Zen free models are promotional and may be withdrawn; switch `llm.model` if one stops responding. Some Zen models are served on `/responses` or `/messages` instead of `/chat/completions`; pick a model listed under chat completions.
- Unsigned builds: Gatekeeper and SmartScreen will warn on first launch.

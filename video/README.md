# Tour video

Everything that produces the STFU feature tour: the script, the narration, the
footage, the Neon edit, the thumbnail and the watch page. It follows the Git Gud,
htmlify and Open Chinese tours and uses the same narrator voice.

| Path | What it is |
| --- | --- |
| `script.json` | The script: sections of lines, each with what is said and a take or an FX component to show. `dictations` holds what the person on screen says and what STFU pastes. `youtube` holds the thumbnail text. |
| `tools/voice.mjs` | Narrates every line with a local Kokoro server (voice `af_heart`), and voices the dictations in a second voice with their loudness every 1/30 s for the pill's waveform. |
| `stage/desk.html`, `stage/stage.ts` | The stage: a Mac or Windows desktop with a mail, notes, chat and Notepad window, the real pill and the real Settings in iframes, and the hotkey-to-paste sequence. |
| `stage/vite.config.mjs` | Serves the app's own `index.html` and `settings.html` with `@tauri-apps/api` swapped for `web-shims/`. |
| `web-shims/` | `invoke` and `listen` for a plain browser. `mock.ts` answers every command the pages call, with the messages from `src-tauri/src/commands.rs`. |
| `tools/director.ts` | Records a page through the DevTools screencast, draws a cursor, and times actions to the narration's words. |
| `tools/takes.ts`, `tools/record.ts` | The takes, and the runner that records them in headless Chromium at 1920x1080. |
| `tools/edit.ts` | Builds the edit in Neon Video Studio and optionally renders it. |
| `tools/neon.ts` | neon-cli against this video's own Neon instance (`~/.neon-video-tour-stfu`). |
| `tools/thumb.ts` | The YouTube thumbnail, made with Neon's thumbnail feature. |
| `tools/page.ts` | Description, chapters, captions, title and tags in `build/publish/`, and the watch page in `~/htmlify/videos/stfu-tour/`. |
| `tools/sheet.mjs`, `tools/check.mjs` | Contact sheets of a take and of the render, and loudness of each dictation against the narration. |
| `fx/stfu-fx` | Neon FX pack in STFU's look: title, chapter label, pipeline diagram, install card, thumbnail. |

## Make it again

Needs a Kokoro server, Neon Video Studio with `neon-cli` linked, Bun, and
Playwright's Chromium in `~/Library/Caches/ms-playwright` (or `CHROMIUM=<path>`).

```sh
pnpm install --ignore-scripts
pnpm --dir video install --ignore-workspace
docker run -d --rm --name kokoro-tours -p 8882:8880 ghcr.io/remsky/kokoro-fastapi-cpu:v0.9.0
node video/tools/voice.mjs                 # narration and dictations
node video/tools/record.ts                 # every take; name takes to redo only those
node video/tools/neon.ts serve --detach
node video/tools/edit.ts --render          # ~/Videos/tours/stfu.neon → build/renders/stfu.mp4
node video/tools/thumb.ts                  # build/thumbs/stfu.png
node video/tools/page.ts                   # build/publish/ and the watch page
node video/tools/check.mjs                 # build/sheets/render.png and loudness
```

## What's real and what's staged

- The pill and Settings are the app's real `index.html` and `settings.html`
  from this checkout, with their real CSS and TypeScript. Only
  `@tauri-apps/api` is replaced.
- The Rust core is stood in for by `web-shims/mock.ts`: config, profiles,
  history, test results, update check and the OpenCode key error use the same
  shapes and messages as the commands, but nothing calls Groq, OpenCode or
  GitHub.
- The desktops, windows, menu bar, tray menu and taskbar are drawn by the
  stage. The tray menu copies the items in `src-tauri/src/lib.rs`.
- The dictations are a second Kokoro voice (`am_michael`). The pill's waveform
  follows that voice's loudness. The cleaned text is written by hand to match
  the cleanup rules in `src-tauri/src/llm.rs`.
- Native select menus do not show in a headless screencast, so the provider and
  model lists are named in callouts instead of opened.

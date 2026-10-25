// Serves STFU's real webviews (index.html, settings.html) in a plain browser, with
// @tauri-apps/api swapped for the shims in video/web-shims, plus the stage page at
// /video/stage/desk.html. Nothing in the app's source changes.
//
//   pnpm exec vite --config video/stage/vite.config.mjs
import { fileURLToPath } from "node:url";

const here = (path) => fileURLToPath(new URL(path, import.meta.url));

export default {
  root: here("../.."),
  clearScreen: false,
  logLevel: "warn",
  resolve: {
    alias: [
      { find: "@tauri-apps/api/core", replacement: here("../web-shims/core.ts") },
      { find: "@tauri-apps/api/event", replacement: here("../web-shims/event.ts") },
    ],
  },
  server: { port: 1430, strictPort: true, watch: { ignored: ["**/src-tauri/**", "**/video/build/**"] } },
};

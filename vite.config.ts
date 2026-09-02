import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;
const rootDir = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig(async ({ mode }) => {
  const stable = mode === "stable";

  return {
    plugins: [react(), tailwindcss()],
    clearScreen: false,
    resolve: {
      alias: {
        "@tauri-apps/api/core": path.resolve(rootDir, "src/bridge/tauriCore.ts"),
        "@tauri-apps/api/event": path.resolve(rootDir, "src/bridge/tauriEvent.ts"),
        "@tauri-apps/api/window": path.resolve(rootDir, "src/bridge/tauriWindow.ts"),
        "@tauri-apps/api/webview": path.resolve(rootDir, "src/bridge/tauriWebview.ts"),
        "@tauri-apps/api/app": path.resolve(rootDir, "src/bridge/tauriApp.ts"),
        "@tauri-apps/plugin-dialog": path.resolve(rootDir, "src/bridge/tauriDialog.ts"),
        "tauri-native-core": path.resolve(
          rootDir,
          "node_modules/@tauri-apps/api/core.js",
        ),
        "tauri-native-event": path.resolve(
          rootDir,
          "node_modules/@tauri-apps/api/event.js",
        ),
        "tauri-native-window": path.resolve(
          rootDir,
          "node_modules/@tauri-apps/api/window.js",
        ),
        "tauri-native-webview": path.resolve(
          rootDir,
          "node_modules/@tauri-apps/api/webview.js",
        ),
        "tauri-native-app": path.resolve(
          rootDir,
          "node_modules/@tauri-apps/api/app.js",
        ),
        "tauri-native-dialog": path.resolve(
          rootDir,
          "node_modules/@tauri-apps/plugin-dialog/dist-js/index.js",
        ),
      },
    },
    server: {
      port: 1420,
      strictPort: true,
      host: host || false,
      hmr: stable
        ? false
        : host
          ? {
              protocol: "ws",
              host,
              port: 1421,
            }
          : undefined,
      watch: {
        ignored: stable ? ["**/*"] : ["**/src-tauri/**"],
      },
    },
  };
});

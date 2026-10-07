import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath } from "node:url";

const OPENER = "@tauri-apps/plugin-opener";
const OPENER_SHIM = fileURLToPath(
  new URL("./src/platform/tauri/opener.ts", import.meta.url),
);

/**
 * Send every app import of the opener plugin through the in-app shim, so
 * links clicked in a terminal can open in the built-in browser. The shim
 * itself still gets the real package.
 */
function routeOpener(): Plugin {
  return {
    name: "monocode:route-opener",
    enforce: "pre",
    resolveId(source, importer) {
      if (source !== OPENER || !importer) return null;
      if (importer.split("?")[0] === OPENER_SHIM) return null;
      return OPENER_SHIM;
    },
  };
}

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async ({ mode }) => {
  const stable = mode === "stable";

  return {
    plugins: [routeOpener(), react(), tailwindcss()],
    clearScreen: false,
    build: {
      rollupOptions: {
        // The quick composer panel loads its own page so it does not boot the
        // whole workspace.
        input: {
          main: "index.html",
          quickComposer: "quick-composer.html",
        },
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

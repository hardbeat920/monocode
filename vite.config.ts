import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

const PDFJS_ROOT = "node_modules/pdfjs-dist";

/**
 * Serve the files pdf.js fetches at runtime from `/pdfjs/`, and copy them into
 * the build. The worker reads standard fonts and CMaps for PDFs that don't
 * embed them, and imports the JavaScript JPEG 2000 and JBIG2 decoders because
 * the CSP doesn't allow compiling the wasm ones.
 */
function pdfjsAssets(): Plugin {
  const files = () => [
    ...readdirSync(join(PDFJS_ROOT, "cmaps")).map((name) => `cmaps/${name}`),
    ...readdirSync(join(PDFJS_ROOT, "standard_fonts")).map(
      (name) => `standard_fonts/${name}`,
    ),
    "wasm/openjpeg_nowasm_fallback.js",
    "wasm/jbig2_nowasm_fallback.js",
  ];

  return {
    name: "pdfjs-assets",
    configureServer(server) {
      const served = new Set(files());
      server.middlewares.use("/pdfjs/", (req, res, next) => {
        const file = decodeURIComponent((req.url ?? "").split("?")[0]).slice(1);
        if (!served.has(file)) return next();
        res.setHeader(
          "Content-Type",
          file.endsWith(".js") ? "text/javascript" : "application/octet-stream",
        );
        res.end(readFileSync(join(PDFJS_ROOT, file)));
      });
    },
    generateBundle() {
      for (const file of files()) {
        this.emitFile({
          type: "asset",
          fileName: `pdfjs/${file}`,
          source: readFileSync(join(PDFJS_ROOT, file)),
        });
      }
    },
  };
}

export default defineConfig(async ({ mode }) => {
  const stable = mode === "stable";

  return {
    plugins: [react(), tailwindcss(), pdfjsAssets()],
    clearScreen: false,
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

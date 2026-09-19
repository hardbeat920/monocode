import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async ({ mode }) => {
  const stable = mode === "stable";

  return {
    plugins: [react(), tailwindcss()],
    clearScreen: false,
    server: {
      port: 1420,
      strictPort: true,
      // Pin IPv4 loopback: with `false` vite binds whatever `localhost`
      // resolves to first (here ::1 only), and webview subresource requests
      // that land on 127.0.0.1 get refused mid-page — a splash that never
      // finishes loading. TAURI_DEV_HOST still wins for device testing.
      host: host || "127.0.0.1",
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

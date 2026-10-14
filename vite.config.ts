import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { optimize } from "@tailwindcss/node";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// Tailwind writes each `color-mix()` it cannot resolve as a full-strength
// fallback, then a nested `@supports` block with the real value. WebKit (Safari
// 18.3) skips a nested `@supports` block that comes after a plain declaration in
// the same rule, so the fallback wins. `vite build` runs Tailwind's Lightning
// CSS pass, which moves these blocks to the top level, but dev does not. Run the
// same pass on Tailwind's dev output so dev renders like the build.
function tailwindcssWithDevOptimize(): Plugin[] {
  return tailwindcss().map((plugin) => {
    if (plugin.name !== "@tailwindcss/vite:generate:serve") return plugin;
    const transform = plugin.transform;
    if (typeof transform !== "object" || transform === null) {
      throw new Error(`${plugin.name} no longer has an object transform hook`);
    }
    const generate = transform.handler;
    return {
      ...plugin,
      transform: {
        ...transform,
        async handler(...args) {
          const result = await generate.apply(this, args);
          if (typeof result !== "object" || result?.code === undefined) {
            return result;
          }
          const map = typeof result.map === "string" ? result.map : undefined;
          return optimize(result.code, { minify: false, map });
        },
      },
    };
  });
}

export default defineConfig(async ({ mode }) => {
  const stable = mode === "stable";

  return {
    plugins: [react(), tailwindcssWithDevOptimize()],
    clearScreen: false,
    build: {
      rollupOptions: {
        // The quick composer panel loads its own page so it does not boot the
        // whole workspace.
        input: {
          main: "index.html",
          quickComposer: "quick-composer.html",
          monoChat: "mono-chat.html",
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

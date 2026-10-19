import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

const root = fileURLToPath(new URL(".", import.meta.url));

export default defineConfig({
  root,
  resolve: {
    alias: {
      "@": `${root}src`,
      react: `${root}node_modules/react`,
      "react-dom": `${root}node_modules/react-dom`,
    },
  },
  esbuild: { jsx: "automatic" },
  test: {
    environment: "happy-dom",
    include: ["tests/**/*.test.ts", "tests/**/*.test.tsx"],
    maxWorkers: 1,
    minWorkers: 1,
  },
});

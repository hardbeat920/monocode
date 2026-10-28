import { defineConfig } from "@playwright/test";
import base from "../../playwright.config";
import { fileURLToPath } from "node:url";

process.env.MONOCODE_PERFORMANCE = "1";

export default defineConfig({
  ...base,
  testDir: ".",
  testMatch: "workspace-performance.spec.ts",
  webServer: {
    cwd: fileURLToPath(new URL("../..", import.meta.url)),
    command: `${process.env.PERFORMANCE_PREBUILT ? "" : `npm run build -- --mode ${process.env.REACT_PROFILE ? "benchmark-profile" : "benchmark"} && `}npx vite preview --host 127.0.0.1 --port 1435`,
    url: "http://127.0.0.1:1435/tests/browser/workspace-performance.html",
    timeout: 180_000,
    reuseExistingServer: false,
  },
});

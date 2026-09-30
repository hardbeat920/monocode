import { defineConfig } from "vitest/config";

const windows = process.platform === "win32";

export default defineConfig({
  test: {
    environment: "node",
    include: ["host/**/*.test.ts"],
    // These integration tests launch real Git, Node and PowerShell processes.
    // Competing suites on Windows runners can exceed the default 5s budget,
    // leaving processes alive when teardown tries to remove their directories.
    fileParallelism: !windows,
    testTimeout: windows ? 30_000 : 5_000,
    hookTimeout: windows ? 30_000 : 10_000,
  },
});

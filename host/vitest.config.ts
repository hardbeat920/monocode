import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    include: ["host/**/*.test.ts"],
    // These integration tests launch real Git, Node and PowerShell processes.
    // Competing suites can exceed the default 5s budget, leaving processes
    // alive when teardown tries to remove their directories.
    // Serial file execution avoids cross-suite contention on Git and child processes.
    fileParallelism: false,
    testTimeout: 30_000,
    hookTimeout: 30_000,
  },
});

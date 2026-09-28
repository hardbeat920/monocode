import { expect, it, vi } from "vitest";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { readFileSync, existsSync } from "node:fs";
import { spawn } from "node:child_process";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { HostChildBackend } from "./child-backend";

it("runs the resolved Claude version fallback in headless mode", async () => {
  const backend = new HostChildBackend({ claude: process.execPath });
  try {
    const version = await backend.invoke<string>("harness_exec", {
      command: process.execPath,
      args: ["--version"],
      binaryProvider: "claude",
      cwd: process.cwd(),
    });
    expect(version.trim()).toBe(process.version);
    await expect(
      backend.invoke("harness_exec", {
        command: process.execPath,
        args: ["-e", "console.log('unsafe')"],
        binaryProvider: "claude",
      }),
    ).rejects.toThrow("Unsupported headless catalog command");
  } finally {
    await backend.close();
  }
});

it("stops a provider's child processes as well as its main process", async () => {
  const directory = mkdtempSync(join(tmpdir(), "monocode-provider-tree-"));
  const file = join(directory, "provider.cjs");
  writeFileSync(
    file,
    `const { spawn } = require('node:child_process');
const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });
console.log(JSON.stringify({ child: child.pid }));
setInterval(() => {}, 1000);
`,
  );
  const backend = new HostChildBackend();
  let descendant: number | undefined;
  const stopListening = await backend.listen<{ line: string }>(
    "harness-stdout",
    ({ payload }) => {
      descendant = JSON.parse(payload.line).child;
    },
  );
  try {
    await backend.invoke("harness_spawn", {
      sessionId: "tree",
      command: file,
      args: [],
      cwd: directory,
    });
    await vi.waitFor(() => expect(descendant).toBeTruthy());
    await backend.kill("tree");
    await vi.waitFor(
      () => expect(() => process.kill(descendant!, 0)).toThrow(),
      { timeout: 5000 },
    );
  } finally {
    stopListening();
    await backend.close();
    if (descendant) {
      try {
        process.kill(descendant, "SIGKILL");
      } catch {
        /* gone */
      }
    }
    rmSync(directory, { recursive: true, force: true });
  }
}, 15_000);

it("stops a provider tree when its host pipe closes unexpectedly", async () => {
  const directory = mkdtempSync(join(tmpdir(), "monocode-provider-crash-"));
  const treeFile = join(directory, "tree.json");
  const providerFile = join(directory, "provider.cjs");
  writeFileSync(
    providerFile,
    `const { spawn } = require('node:child_process');
const { writeFileSync } = require('node:fs');
const descendant = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });
writeFileSync(${JSON.stringify(treeFile)}, JSON.stringify({ provider: process.pid, descendant: descendant.pid }));
setInterval(() => {}, 1000);
`,
  );
  const guard = spawn(
    process.execPath,
    [resolve("build/host/provider-guard.mjs"), process.execPath, providerFile],
    {
      cwd: directory,
      stdio: ["pipe", "ignore", "ignore", "pipe"],
      detached: process.platform !== "win32",
      windowsHide: true,
    },
  );
  let tree: { provider: number; descendant: number } | undefined;
  try {
    await vi.waitFor(() => expect(existsSync(treeFile)).toBe(true));
    tree = JSON.parse(readFileSync(treeFile, "utf8"));
    guard.stdio[3]?.destroy();
    await vi.waitFor(
      () => {
        expect(() => process.kill(tree!.provider, 0)).toThrow();
        expect(() => process.kill(tree!.descendant, 0)).toThrow();
      },
      { timeout: 5_000 },
    );
  } finally {
    guard.stdio[3]?.destroy();
    guard.kill("SIGKILL");
    for (const pid of [tree?.provider, tree?.descendant]) {
      if (pid)
        try {
          process.kill(pid, "SIGKILL");
        } catch {
          /* gone */
        }
    }
    rmSync(directory, { recursive: true, force: true });
  }
}, 10_000);

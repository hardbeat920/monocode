import { expect, it, vi } from "vitest";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { HostChildBackend } from "./child-backend";

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

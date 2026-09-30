import { expect, it } from "vitest";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { acquireHostOwner } from "./owner";

it("rejects simultaneous owners and recovers a stale modern PID lock", async () => {
  const directory = mkdtempSync(join(tmpdir(), "monocode-owner-"));
  let release: (() => void) | undefined;
  try {
    release = await acquireHostOwner(directory);
    await expect(acquireHostOwner(directory)).rejects.toThrow("already owns");
    release();
    release = undefined;
    writeFileSync(
      join(directory, "owner.lock"),
      JSON.stringify({ version: 2, pid: process.pid }),
    );
    release = await acquireHostOwner(directory);
  } finally {
    release?.();
    rmSync(directory, { recursive: true, force: true });
  }
});

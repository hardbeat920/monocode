import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it } from "vitest";
import {
  MAX_CONCURRENT_GIT,
  __resetGitBinaryForTest,
  execGit,
  getGitExecStats,
  isExecutableNativeBinary,
  resetGitExecStats,
  resolveGitBinary,
  withGitSlot,
} from "./git-exec";

const dirs: string[] = [];
afterEach(() => {
  delete process.env.MONOCODE_GIT_BIN;
  __resetGitBinaryForTest();
  for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});

it("rejects shell-script wrappers but accepts native executables", () => {
  const dir = mkdtempSync(join(tmpdir(), "monocode-git-exec-"));
  dirs.push(dir);
  const script = join(dir, "git");
  writeFileSync(script, "#!/bin/bash\nexec /usr/bin/git \"$@\"\n");
  chmodSync(script, 0o755);
  expect(isExecutableNativeBinary(script)).toBe(false);
  expect(isExecutableNativeBinary(process.execPath)).toBe(true);
  expect(isExecutableNativeBinary(join(dir, "missing"))).toBe(false);
});

it.runIf(process.platform === "darwin")(
  "resolves a real macOS git binary instead of PATH wrappers or the Xcode shim",
  () => {
    const git = resolveGitBinary();
    expect(git).not.toBe("git");
    expect(isExecutableNativeBinary(git)).toBe(true);
    expect(resolveGitBinary()).toBe(git);
  },
);

it("honors MONOCODE_GIT_BIN and resets cleanly", () => {
  process.env.MONOCODE_GIT_BIN = "/custom/git";
  __resetGitBinaryForTest();
  expect(resolveGitBinary()).toBe("/custom/git");
  delete process.env.MONOCODE_GIT_BIN;
  __resetGitBinaryForTest();
  expect(resolveGitBinary()).not.toBe("/custom/git");
});

it("caps concurrent git work and counts spawns", async () => {
  resetGitExecStats();
  await Promise.all(
    Array.from({ length: 12 }, () => execGit(tmpdir(), ["--version"])),
  );
  expect(getGitExecStats()).toEqual({
    spawned: 12,
    peak: MAX_CONCURRENT_GIT,
  });
  // The slot is released even when the work fails.
  await expect(withGitSlot(async () => Promise.reject(new Error("x")))).rejects.toThrow("x");
  expect(await withGitSlot(async () => 1)).toBe(1);
});

it("expires queued callers at their deadline without waiting for held slots", async () => {
  const release: (() => void)[] = [];
  const holders = Array.from({ length: MAX_CONCURRENT_GIT }, () =>
    withGitSlot(
      () =>
        new Promise<void>((resolve) => {
          release.push(resolve);
        }),
    ),
  );
  // Let all slots enter the active section.
  await new Promise((r) => setTimeout(r, 10));
  const started = Date.now();
  await expect(
    withGitSlot(async () => "never", Date.now() + 40),
  ).rejects.toThrow(/deadline exceeded/);
  expect(Date.now() - started).toBeLessThan(500);
  for (const done of release) done();
  await Promise.all(holders);
});

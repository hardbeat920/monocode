import { expect, it, vi } from "vitest";
import {
  chmodSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { resolveProvider, providerLaunch } from "./process";

it("runs the standard Windows npm OpenCode entry without interpreting its wrapper", async () => {
  const directory = mkdtempSync(join(tmpdir(), "monocode-opencode-launch-"));
  const entry = join(directory, "node_modules/opencode-ai/bin/opencode");
  mkdirSync(join(directory, "node_modules/opencode-ai/bin"), {
    recursive: true,
  });
  writeFileSync(entry, "process.exit(0)");
  try {
    expect(
      await providerLaunch(
        join(directory, "opencode.cmd"),
        ["serve", "a & b"],
        "win32",
      ),
    ).toEqual({ command: process.execPath, args: [entry, "serve", "a & b"] });
    await expect(
      providerLaunch(join(directory, "custom.cmd"), [], "win32"),
    ).rejects.toThrow("Unsupported Windows provider launcher");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

it.each(["cursor", "pi", "fx"] as const)(
  "does not execute an unrelated ambiguous %s binary while resolving providers",
  async (provider) => {
    const directory = mkdtempSync(
      join(tmpdir(), "monocode-provider-identity-"),
    );
    const name = provider === "cursor" ? "agent" : provider;
    const candidate = join(directory, name);
    const sentinel = join(directory, "executed");
    writeFileSync(candidate, `#!/bin/sh\nprintf bad > '${sentinel}'\n`);
    chmodSync(candidate, 0o755);
    vi.stubEnv("PATH", directory);
    try {
      let resolved: string | undefined;
      try {
        resolved = await resolveProvider(provider);
      } catch {
        /* no matching provider is expected on CI */
      }
      expect(resolved).not.toBe(candidate);
      expect(existsSync(sentinel)).toBe(false);
    } finally {
      vi.unstubAllEnvs();
      rmSync(directory, { recursive: true, force: true });
    }
  },
);

it.runIf(process.platform !== "win32")(
  "recognizes a Cursor agent shim without executing it",
  async () => {
    const directory = mkdtempSync(join(tmpdir(), "monocode-cursor-identity-"));
    const targetDirectory = join(directory, "cursor-agent-package");
    const target = join(targetDirectory, "cursor-agent");
    const candidate = join(directory, "agent");
    const sentinel = join(directory, "executed");
    mkdirSync(targetDirectory);
    writeFileSync(target, `#!/bin/sh\nprintf bad > '${sentinel}'\n`);
    chmodSync(target, 0o755);
    symlinkSync(target, candidate);
    vi.stubEnv("PATH", directory);
    try {
      expect(await resolveProvider("cursor")).toBe(candidate);
      expect(existsSync(sentinel)).toBe(false);
    } finally {
      vi.unstubAllEnvs();
      rmSync(directory, { recursive: true, force: true });
    }
  },
);

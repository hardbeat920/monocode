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
import { resolveProvider } from "./process";

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
  "recognizes an npm-installed pi launcher stub through its package manifest",
  async () => {
    const directory = mkdtempSync(join(tmpdir(), "monocode-pi-npm-"));
    const packageDirectory = join(
      directory,
      "lib/node_modules/@earendil-works/pi-coding-agent/dist/bundle",
    );
    const stub = join(packageDirectory, "cli.js");
    mkdirSync(packageDirectory, { recursive: true });
    // Mirrors the real npm launcher: a thin stub whose content carries none
    // of the marker strings — identity only lives in the package manifest.
    writeFileSync(
      stub,
      '#!/usr/bin/env node\nimport { createRequire } from "node:module";\n\nenableCompileCache();\ncreateRequire(import.meta.url)("./cli-runtime.js");\n',
    );
    chmodSync(stub, 0o755);
    writeFileSync(
      join(packageDirectory, "../../package.json"),
      JSON.stringify({ name: "@earendil-works/pi-coding-agent" }),
    );
    const candidate = join(directory, "bin/pi");
    mkdirSync(join(directory, "bin"), { recursive: true });
    symlinkSync(stub, candidate);
    vi.stubEnv("PATH", join(directory, "bin"));
    try {
      expect(await resolveProvider("pi")).toBe(candidate);
    } finally {
      vi.unstubAllEnvs();
      rmSync(directory, { recursive: true, force: true });
    }
  },
);

it.runIf(process.platform !== "win32")(
  "does not mistake an unrelated npm stub named pi for the pi agent",
  async () => {
    const directory = mkdtempSync(join(tmpdir(), "monocode-pi-unrelated-"));
    const packageDirectory = join(
      directory,
      "lib/node_modules/pi-coding-agent-tools/dist",
    );
    mkdirSync(packageDirectory, { recursive: true });
    writeFileSync(join(packageDirectory, "cli.js"), "#!/usr/bin/env node\n");
    chmodSync(join(packageDirectory, "cli.js"), 0o755);
    writeFileSync(
      join(packageDirectory, "../../package.json"),
      JSON.stringify({ name: "pi-coding-agent-tools" }),
    );
    const candidate = join(directory, "bin/pi");
    mkdirSync(join(directory, "bin"), { recursive: true });
    symlinkSync(join(packageDirectory, "cli.js"), candidate);
    vi.stubEnv("PATH", join(directory, "bin"));
    try {
      const resolved = await resolveProvider("pi").catch(() => undefined);
      expect(resolved).not.toBe(candidate);
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

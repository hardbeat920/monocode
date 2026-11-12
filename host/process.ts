import { createReadStream } from "node:fs";
import { access, readFile, realpath, readlink, stat } from "node:fs/promises";
import { constants } from "node:fs";
import { homedir } from "node:os";
import {
  delimiter,
  dirname,
  extname,
  join,
  basename,
  isAbsolute,
} from "node:path";
import type { RemoteProvider } from "../src/features/connections/model/protocol";

const npmEntries: Record<string, string> = {
  codex: "node_modules/@openai/codex/bin/codex.js",
  claude: "node_modules/@anthropic-ai/claude-code/cli.js",
};

const binaryNames: Record<RemoteProvider, string[]> = {
  codex: ["codex"],
  claude: ["claude"],
  cursor: ["cursor-agent", "agent"],
  grok: ["grok"],
  opencode: ["opencode"],
  pi: ["pi-coding-agent", "pi"],
  omp: ["omp"],
  fx: ["fx"],
  hermes: ["hermes"],
  antigravity:
    process.platform === "win32"
      ? ["agy_acp_server", "agy"]
      : ["agy_acp_server.par", "agy"],
  devin: ["devin"],
};

/** Collect PATH and standard provider installation directories for discovery. */
const providerDirectories = (provider: RemoteProvider): string[] => {
  const home = homedir();
  const extra: Partial<Record<RemoteProvider, string[]>> = {
    claude: [
      join(home, ".claude", "local"),
      join(home, ".local", "share", "claude"),
    ],
    grok: [join(home, ".grok", "bin")],
    opencode: [join(home, ".opencode", "bin")],
    fx: [join(home, ".fx", "bin")],
    hermes: [
      join(home, ".hermes", "hermes-agent", "venv", "bin"),
      join(home, ".hermes", "hermes-agent", ".venv", "bin"),
    ],
    antigravity: [
      join(home, ".local", "share", "agy-acp"),
      ...(process.platform === "win32"
        ? [
            join(
              process.env.LOCALAPPDATA ?? join(home, "AppData", "Local"),
              "agy-acp",
            ),
            join(
              process.env.LOCALAPPDATA ?? join(home, "AppData", "Local"),
              "agy",
              "bin",
            ),
          ]
        : []),
    ],
    devin: [
      process.platform === "win32"
        ? join(
            process.env.LOCALAPPDATA ?? join(home, "AppData", "Local"),
            "devin",
            "cli",
            "bin",
          )
        : join(home, ".local", "bin"),
    ],
  };
  return [
    ...new Set([
      ...(process.env.PATH ?? "").split(delimiter),
      join(home, ".local", "bin"),
      join(home, ".npm-global", "bin"),
      join(home, ".cargo", "bin"),
      join(home, "n", "bin"),
      join(home, ".bun", "bin"),
      ...(extra[provider] ?? []),
      ...(process.platform === "win32"
        ? [join(process.env.APPDATA ?? join(home, "AppData", "Roaming"), "npm")]
        : ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/snap/bin"]),
    ]),
  ];
};

/** Verify that an ambiguous launcher name belongs to requested provider. */
async function matchesProvider(
  candidate: string,
  provider: RemoteProvider,
  name: string,
): Promise<boolean> {
  if (provider === "cursor" && name === "agent") {
    try {
      if ((await readlink(candidate)).toLowerCase().includes("cursor-agent"))
        return true;
    } catch {
      /* not a symlink */
    }
    return fileContains(candidate, ["cursor-agent"], 64 * 1024);
  }
  if (provider === "pi" && name === "pi") return isPiLaunchCandidate(candidate);
  if (provider === "fx")
    return fileContains(candidate, [
      "vercel-labs/fx",
      "fx_model",
      "createfxagent",
      "fx acp",
    ]);
  return true;
}

/** Scan file content for any marker, stopping after optional byte limit. */
async function fileContains(
  path: string,
  markers: string[],
  maxBytes = Number.POSITIVE_INFINITY,
): Promise<boolean> {
  let read = 0;
  let carry = "";
  for await (const chunk of createReadStream(path, {
    highWaterMark: 64 * 1024,
  })) {
    const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
    const length = Math.min(bytes.length, maxBytes - read);
    const text = (
      carry + bytes.subarray(0, length).toString("latin1")
    ).toLowerCase();
    if (markers.some((marker) => text.includes(marker))) return true;
    carry = text.slice(-64);
    read += length;
    if (read >= maxBytes) break;
  }
  return false;
}

/** Marker strings that identify a Pi coding agent install. */
const PI_MARKERS = [
  "pi-coding-agent",
  "@earendil-works/pi",
  "@mariozechner/pi-coding-agent",
  "pi_coding_agent",
];

const PI_PACKAGE_NAMES = new Set([
  "@earendil-works/pi-coding-agent",
  "@mariozechner/pi-coding-agent",
]);

/**
 * npm's bin launcher for pi is a thin `#!/usr/bin/env node` stub whose only
 * job is to `createRequire(...)("./cli-runtime.js")` — the marker strings live
 * megabytes deeper in `dist/bundle/chunks/*.js`, past any reasonable head
 * scan. Identify the enclosing package instead: resolve symlinks (npm bins
 * symlink into `lib/node_modules/<pkg>/...`), then walk up to the nearest
 * `package.json` and check its exact `name` against supported Pi packages.
 */
async function isPiLaunchCandidate(candidate: string): Promise<boolean> {
  if (await fileContains(candidate, PI_MARKERS, 64 * 1024)) return true;
  let real: string;
  try {
    real = await realpath(candidate);
  } catch {
    return false;
  }
  let dir = dirname(real);
  // A scoped package's package.json sits two directories up from a nested
  // bin/script; a few hops bound the walk without leaving the package.
  for (let hop = 0; hop < 6; hop += 1) {
    const manifest = join(dir, "package.json");
    try {
      const pkg = JSON.parse(await readFile(manifest, "utf8")) as {
        name?: string;
      };
      const name = pkg.name?.toLowerCase();
      return name !== undefined && PI_PACKAGE_NAMES.has(name);
    } catch {
      /* not a package root; keep walking */
    }
    const parent = dirname(dir);
    if (parent === dir) return false;
    dir = parent;
  }
  return false;
}

/** Resolve executable launcher for remote provider without running it. */
export async function resolveProvider(
  provider: RemoteProvider,
): Promise<string> {
  const windows = process.platform === "win32";
  const directories = providerDirectories(provider).filter(Boolean);
  // Keep the standalone runtime's existing configuration when both are installed.
  const candidates =
    provider === "antigravity"
      ? binaryNames[provider].flatMap((name) =>
          directories.map((directory) => ({ directory, name })),
        )
      : directories.flatMap((directory) =>
          binaryNames[provider].map((name) => ({ directory, name })),
        );
  for (const { directory, name } of candidates) {
    for (const extension of windows ? [".exe", ".cmd", ".bat", ".com"] : [""]) {
      const candidate = join(directory.replace(/^"|"$/g, ""), name + extension);
      try {
        await access(candidate, windows ? constants.F_OK : constants.X_OK);
        if (!(await stat(candidate)).isFile()) continue;
        await providerLaunch(candidate, []);
        if (!(await matchesProvider(candidate, provider, name))) continue;
        return candidate;
      } catch {
        /* try the next installed launcher */
      }
    }
  }
  throw new Error(
    `${provider} is not installed on this host or is missing from its PATH. Install its native CLI or standard npm package.`,
  );
}

/** The CLI embeds ACP; only the standalone Linux runtime needs --uid=. */
export function antigravityArgs(
  command: string,
  platform = process.platform,
): string[] {
  return /^agy(?:\.(?:exe|cmd|bat|com))?$/i.test(basename(command))
    ? ["acp"]
    : platform === "linux"
      ? ["--uid="]
      : [];
}

/** Resolve a one-command agy batch forwarder without evaluating shell text.
 * Only a quoted/literal absolute target or %~dp0-relative agy.exe/agy.com plus
 * %* is accepted, optionally preceded by echo off. Other batch logic is rejected. */
async function antigravityWrapperTarget(command: string): Promise<string> {
  if ((await stat(command)).size > 16 * 1024)
    throw new Error("Unsupported Antigravity Windows wrapper");
  const lines = (await readFile(command, "utf8"))
    .replace(/^\uFEFF/, "")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line && !/^@?echo\s+off$/i.test(line));
  const forward =
    lines.length === 1
      ? /^@?(?:"([^"\r\n]+)"|([^\s"\r\n]+))\s+%\*$/.exec(lines[0])
      : null;
  const target = forward?.[1] ?? forward?.[2];
  if (!target) throw new Error("Unsupported Antigravity Windows wrapper");
  const relative = /^%~dp0(.*)$/i.exec(target);
  const native = relative ? join(dirname(command), relative[1]) : target;
  if (
    native.includes("%") ||
    !isAbsolute(native) ||
    !/^agy\.(exe|com)$/i.test(basename(native))
  )
    throw new Error("Unsupported Antigravity Windows wrapper target");
  if (!(await stat(native)).isFile())
    throw new Error("Missing Antigravity Windows wrapper target");
  return native;
}

/** Spawn known npm entry points or constrained agy native forwarders directly.
 * Custom .cmd/.bat files are never interpreted as shell text; arguments stay literal. */
export async function providerLaunch(
  command: string,
  args: string[],
  platform = process.platform,
): Promise<{ command: string; args: string[] }> {
  if (platform === "win32" && /\.(cmd|bat)$/i.test(command)) {
    const provider = basename(command, extname(command)).toLowerCase();
    if (provider === "agy")
      return { command: await antigravityWrapperTarget(command), args };
    const relative = npmEntries[provider];
    if (!relative) throw new Error("Unsupported Windows provider launcher");
    const entry = join(dirname(command), relative);
    if (!(await stat(entry)).isFile())
      throw new Error("Missing npm provider entry point");
    return { command: process.execPath, args: [entry, ...args] };
  }
  if (/\.(cjs|mjs|js)$/i.test(command))
    return { command: process.execPath, args: [command, ...args] };
  return { command, args };
}

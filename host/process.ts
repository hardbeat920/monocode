import { createReadStream } from "node:fs";
import { access, readlink, stat } from "node:fs/promises";
import { constants } from "node:fs";
import { homedir } from "node:os";
import { delimiter, dirname, extname, join, basename } from "node:path";
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
  droid: ["droid"],
  antigravity: ["agy_acp_server.par"],
};

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
    droid: [join(home, ".factory", "bin"), join(home, "bin")],
    antigravity: [join(home, ".local", "share", "agy-acp")],
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
  if (provider === "pi" && name === "pi")
    return fileContains(
      candidate,
      [
        "pi-coding-agent",
        "@earendil-works/pi",
        "@mariozechner/pi-coding-agent",
        "pi_coding_agent",
      ],
      64 * 1024,
    );
  if (provider === "fx")
    return fileContains(candidate, [
      "vercel-labs/fx",
      "fx_model",
      "createfxagent",
      "fx acp",
    ]);
  return true;
}

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

export async function resolveProvider(
  provider: RemoteProvider,
): Promise<string> {
  const windows = process.platform === "win32";
  if (windows && provider === "antigravity")
    throw new Error("Antigravity ACP is not available on Windows");
  for (const directory of providerDirectories(provider)) {
    if (!directory) continue;
    for (const name of binaryNames[provider]) {
      for (const extension of windows
        ? [".exe", ".cmd", ".bat", ".com"]
        : [""]) {
        const candidate = join(
          directory.replace(/^"|"$/g, ""),
          name + extension,
        );
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
  }
  throw new Error(
    `${provider} is not installed on this host or is missing from its PATH. Install its native CLI or standard npm package.`,
  );
}

/** npm's Windows .cmd wrappers cannot be spawned directly. Run their known
 * package entry point with the bundled Node, preserving argv without a shell.
 * Custom .cmd/.bat wrappers are deliberately not interpreted as shell text. */
export async function providerLaunch(
  command: string,
  args: string[],
  platform = process.platform,
): Promise<{ command: string; args: string[] }> {
  if (platform === "win32" && /\.(cmd|bat)$/i.test(command)) {
    const provider = basename(command, extname(command)).toLowerCase();
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

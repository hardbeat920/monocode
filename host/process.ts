import { access, stat } from "node:fs/promises";
import { constants } from "node:fs";
import { homedir } from "node:os";
import { delimiter, dirname, extname, join, basename } from "node:path";

const npmEntries: Record<string, string> = {
  codex: "node_modules/@openai/codex/bin/codex.js",
  claude: "node_modules/@anthropic-ai/claude-code/cli.js",
};

export async function resolveProvider(
  provider: "codex" | "claude",
): Promise<string> {
  const windows = process.platform === "win32";
  const paths = [
    ...new Set([
      ...(process.env.PATH ?? "").split(delimiter),
      join(homedir(), ".local", "bin"),
      ...(windows
        ? [
            join(
              process.env.APPDATA ?? join(homedir(), "AppData", "Roaming"),
              "npm",
            ),
          ]
        : ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"]),
    ]),
  ];
  for (const directory of paths) {
    if (!directory) continue;
    for (const extension of windows ? [".exe", ".cmd", ".bat", ".com"] : [""]) {
      const candidate = join(
        directory.replace(/^"|"$/g, ""),
        provider + extension,
      );
      try {
        await access(candidate, windows ? constants.F_OK : constants.X_OK);
        if (!(await stat(candidate)).isFile()) continue;
        await providerLaunch(candidate, []);
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

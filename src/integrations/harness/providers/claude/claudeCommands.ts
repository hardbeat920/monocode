import {
  killChild,
  resolveClaudeBinary,
  spawnChild,
  unwatchChild,
  watchChild,
  writeChild,
} from "../../core/child";
import {
  nativeCommandInvocation,
  nativeCommandPrompt,
  type NativeCommand,
  type NativeCommandProvider,
} from "../../core/nativeCommands";
import {
  asRecord,
  buildClaudeSpawnArgs,
  buildControlRequest,
  parseControlResponse,
  parseJsonLine,
} from "./claudeProtocol";
import { pathKey } from "../../../../shared/lib/paths";

const INIT_REQUEST_ID = "monocode_commands_init";
const DISCOVERY_TIMEOUT_MS = 15_000;
/** A send waits this long at most to learn whether its `/name` is a command. */
const SEND_PROBE_TIMEOUT_MS = 5_000;
/** How long a probe's "not a command" answer for a name is trusted. */
const ABSENT_TTL_MS = 30_000;

/**
 * Commands that change session state MonoCode tracks itself (the thread,
 * model, effort, title, usage), plus CLI internals. Commands that only share
 * a name with MonoCode's own are namespaced instead, like omp's.
 */
const HIDDEN_COMMANDS = new Set([
  "clear",
  "color",
  "config",
  "effort",
  "fast",
  "heapdump",
  "model",
  "rename",
  "usage",
  "workflow-launch-exec",
]);

/**
 * Names and aliases the CLI last reported per project and account, including
 * hidden ones: skills and plugins differ between them.
 */
const knownCommands = new Map<string, Set<string>>();
/** Per project and account: names a probe just confirmed absent, and when. */
const absentCommands = new Map<string, Map<string, number>>();
/** Per project and account: when a pre-send probe last failed. */
const failedProbes = new Map<string, number>();
/** Bumped whenever a list is saved, so a late failure can't drop a newer one. */
const listVersions = new Map<string, number>();

function commandsKey(cwd: string, accountId: string | undefined): string {
  return `${accountId ?? "default"}\0${pathKey(cwd)}`;
}

export const claudeCommandProvider: NativeCommandProvider = {
  monocodeSkills: true,
  discover: ({ cwd, accountId }) => discoverClaudeCommands(cwd, accountId),
  beforeSend: refreshLeadingCommand,
};

/**
 * The command a prompt starts with, from the last probe of this project and
 * account. Unprobed, any leading `/name` counts rather than risk burying it.
 */
export function leadingClaudeCommand(
  text: string,
  context: { cwd: string; accountId?: string },
): string | null {
  const name = leadingName(text);
  if (!name) return null;
  const known = knownCommands.get(commandsKey(context.cwd, context.accountId));
  return !known || known.has(name) ? name : null;
}

/**
 * Only Ultrathink's prompt prefix can bury a command. A leading name the last
 * probe didn't report may be a skill added since, so ask the CLI again; a
 * failed probe forgets the stale list, so the name then counts as a command.
 */
async function refreshLeadingCommand(
  text: string,
  context: { cwd: string; accountId?: string; effort?: string },
): Promise<void> {
  const name = leadingName(text);
  if (context.effort !== "ultrathink" || !name) return;
  const key = commandsKey(context.cwd, context.accountId);
  if (knownCommands.get(key)?.has(name)) return;
  const absentAt = absentCommands.get(key)?.get(name);
  if (absentAt !== undefined && Date.now() - absentAt < ABSENT_TTL_MS) return;
  const failedAt = failedProbes.get(key);
  if (failedAt !== undefined && Date.now() - failedAt < ABSENT_TTL_MS) return;
  const version = listVersions.get(key);
  try {
    await discoverClaudeCommands(
      context.cwd,
      context.accountId,
      SEND_PROBE_TIMEOUT_MS,
    );
  } catch {
    failedProbes.set(key, Date.now());
    if (listVersions.get(key) === version) knownCommands.delete(key);
    return;
  }
  failedProbes.delete(key);
  const absent = absentCommands.get(key) ?? new Map<string, number>();
  absentCommands.set(key, absent);
  if (knownCommands.get(key)?.has(name)) absent.delete(name);
  else absent.set(name, Date.now());
}

/** A namespaced pick such as `/claude:compact` reaches the CLI as `/compact`. */
export function claudePromptText(text: string): string {
  return nativeCommandPrompt("claude", text);
}

function leadingName(text: string): string | undefined {
  return /^\s*\/([^\s/\\]+)(?=\s|$)/.exec(claudePromptText(text))?.[1];
}

/** Ask a throwaway CLI in `cwd` for the commands its `initialize` reports. */
export async function discoverClaudeCommands(
  cwd: string,
  accountId?: string,
  timeoutMs = DISCOVERY_TIMEOUT_MS,
): Promise<NativeCommand[]> {
  const { path } = await resolveClaudeBinary();
  const childId = `monocode-claude-commands-${crypto.randomUUID()}`;

  let settle: {
    resolve: (commands: NativeCommand[]) => void;
    reject: (error: Error) => void;
  } | null = null;
  const pending = new Promise<NativeCommand[]>((resolve, reject) => {
    settle = { resolve, reject };
  });
  // The CLI can exit while spawn or write is still pending; the race below
  // reports that failure, so it must not also surface as unhandled.
  pending.catch(() => undefined);

  watchChild(
    childId,
    (line) => {
      const rec = parseJsonLine(line);
      const response = rec ? parseControlResponse(rec) : null;
      if (response?.requestId !== INIT_REQUEST_ID) return;
      if (!response.ok) {
        settle?.reject(new Error(response.error ?? "initialize failed"));
        return;
      }
      try {
        rememberCommands(commandsKey(cwd, accountId), response.payload);
        settle?.resolve(claudeCommandsFromInitialize(response.payload));
      } catch (error) {
        settle?.reject(error instanceof Error ? error : new Error(String(error)));
      }
    },
    () => settle?.reject(new Error("Claude Code command probe exited")),
  );

  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    // Same setting sources as a live session so project, user, and plugin
    // commands match, but no hooks, MCP servers, or saved transcript.
    const args = buildClaudeSpawnArgs({
      settings: { disableAllHooks: true },
      includePartialMessages: false,
    });
    args.push(
      "--no-session-persistence",
      "--strict-mcp-config",
      "--mcp-config",
      JSON.stringify({ mcpServers: {} }),
    );
    await spawnChild(
      childId,
      path,
      args,
      cwd,
      { provider: "claude", id: accountId ?? "default" },
      "claude",
    );
    await writeChild(
      childId,
      JSON.stringify(
        buildControlRequest(INIT_REQUEST_ID, { subtype: "initialize" }),
      ),
    );
    return await Promise.race([
      pending,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("Claude Code command probe timed out")),
          timeoutMs,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
    unwatchChild(childId);
    await killChild(childId).catch(() => undefined);
  }
}

export function claudeCommandsFromInitialize(
  payload: Record<string, unknown> | null,
): NativeCommand[] {
  const commands = payload?.commands;
  if (!Array.isArray(commands))
    throw new Error("Claude Code initialize returned no commands array");
  const seen = new Set<string>();
  return commands.flatMap((value): NativeCommand[] => {
    const row = asRecord(value);
    const name = row?.name;
    if (
      typeof name !== "string" ||
      !isCommandName(name) ||
      name.startsWith("__") ||
      HIDDEN_COMMANDS.has(name) ||
      seen.has(name)
    )
      return [];
    seen.add(name);
    const aliases = Array.isArray(row?.aliases)
      ? row.aliases.filter(
          (alias): alias is string =>
            typeof alias === "string" &&
            isCommandName(alias) &&
            !HIDDEN_COMMANDS.has(alias),
        )
      : [];
    const hint = row?.argumentHint;
    return [
      {
        name,
        invocation: nativeCommandInvocation("claude", name),
        source: "claude",
        description:
          typeof row?.description === "string" ? row.description : "",
        ...(row?.builtin === true ? { origin: "builtin" } : {}),
        ...(aliases.length ? { aliases } : {}),
        ...(typeof hint === "string" && hint ? { inputHint: hint } : {}),
      },
    ];
  });
}

function rememberCommands(
  key: string,
  payload: Record<string, unknown> | null,
): void {
  if (!Array.isArray(payload?.commands)) return;
  const names = new Set<string>();
  for (const value of payload.commands) {
    const row = asRecord(value);
    for (const name of [
      row?.name,
      ...(Array.isArray(row?.aliases) ? row.aliases : []),
    ]) {
      if (typeof name === "string" && isCommandName(name)) names.add(name);
    }
  }
  knownCommands.set(key, names);
  listVersions.set(key, (listVersions.get(key) ?? 0) + 1);
}

function isCommandName(name: string): boolean {
  return !!name && !/[\s/\\]/.test(name);
}

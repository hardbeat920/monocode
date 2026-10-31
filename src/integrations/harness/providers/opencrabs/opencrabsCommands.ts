import {
  nativeCommandInvocation,
  type NativeCommand,
  type NativeCommandProvider,
} from "../../core/nativeCommands";

/** Slash commands pushed by the server, cached per MonoCode thread. */
const commandsByThread = new Map<string, NativeCommand[]>();
const commandSubscribers = new Map<string, Set<(c: NativeCommand[]) => void>>();

export function cacheNativeCommands(
  threadId: string,
  rows: { name: string; description: string }[],
): void {
  const commands: NativeCommand[] = rows.map((row) => ({
    name: row.name,
    description: row.description,
    invocation: nativeCommandInvocation("opencrabs", row.name),
    source: "opencrabs" as const,
  }));
  commandsByThread.set(threadId, commands);
  commandSubscribers.get(threadId)?.forEach((cb) => cb(commands));
}

export function clearNativeCommands(threadId: string): void {
  commandsByThread.delete(threadId);
  commandSubscribers.delete(threadId);
}

/**
 * The server's `available_commands_update` push, surfaced as a command
 * provider: built-ins, skills, and the user's own commands.toml entries are
 * slash-able from the picker once a session is live.
 */
export const openCrabsCommands: NativeCommandProvider = {
  discover: async (context) =>
    commandsByThread.get(context.sessionId ?? "") ?? [],
  subscribe: (context, onCommands) => {
    const key = context.sessionId ?? "";
    const set = commandSubscribers.get(key) ?? new Set();
    set.add(onCommands);
    commandSubscribers.set(key, set);
    const cached = commandsByThread.get(key);
    if (cached) onCommands(cached);
    return () => {
      set.delete(onCommands);
    };
  },
  rawSlashCommands: true,
};

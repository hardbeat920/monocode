import type { HarnessId } from "../../../features/sessions/model/session";

/** Provider-owned commands share a picker, but execute inside their harness. */
export type NativeCommand = {
  name: string;
  description: string;
  invocation: string;
  source: HarnessId;
  origin?: string;
  aliases?: string[];
  inputHint?: string;
  subcommands?: Array<{ name: string; description?: string; usage?: string }>;
};

export type CommandContext = {
  cwd: string;
  sessionId?: string;
  /** Provider account whose config the session runs under. */
  accountId?: string;
};

export type NativeCommandProvider = {
  discover(context: CommandContext): Promise<NativeCommand[]>;
  subscribe?(
    context: CommandContext,
    onCommands: (commands: NativeCommand[]) => void,
  ): () => void;
  /** Full command runtimes own slash arguments, including @file-like text. */
  rawSlashCommands?: boolean;
  /**
   * The harness reads the same skill files MonoCode discovers, so MonoCode's
   * skill settings apply: disabled skills stay hidden and `/create-skill` is
   * offered.
   */
  monocodeSkills?: boolean;
  /**
   * Runs while a new turn's prompt is prepared, before it reaches the harness,
   * so a stop meanwhile still wins. Steers skip it: a wait there could outlast
   * the turn they target.
   */
  beforeSend?(text: string, context: CommandContext & { effort?: string }): Promise<void>;
};

/** MonoCode's own command names; a native command using one is namespaced. */
const RESERVED_COMMANDS = new Set([
  "add-to-folder",
  "btw",
  "compact",
  "draft",
  "mcp",
  "orchestrator",
  "plan",
]);

export function nativeCommandInvocation(
  harness: HarnessId,
  name: string,
): string {
  return RESERVED_COMMANDS.has(name) ? `${harness}:${name}` : name;
}

/** Only our reserved-command escape is rewritten; custom names remain exact. */
export function nativeCommandPrompt(harness: HarnessId, text: string): string {
  return text.replace(
    /^(\s*)\/([^\s]+)/,
    (whole, space: string, name: string) => {
      const prefix = `${harness}:`;
      return name.startsWith(prefix) &&
        RESERVED_COMMANDS.has(name.slice(prefix.length))
        ? `${space}/${name.slice(prefix.length)}`
        : whole;
    },
  );
}

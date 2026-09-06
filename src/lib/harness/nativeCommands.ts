import type { HarnessId } from "../session";

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

export type CommandContext = { cwd: string; sessionId?: string };

export type NativeCommandProvider = {
  discover(context: CommandContext): Promise<NativeCommand[]>;
  subscribe?(
    context: CommandContext,
    onCommands: (commands: NativeCommand[]) => void,
  ): () => void;
  /** Full command runtimes own slash arguments, including @file-like text. */
  rawSlashCommands?: boolean;
};

const RESERVED_COMMANDS = new Set(["plan", "compact"]);

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

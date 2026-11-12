import type { Block, Session, ShellCommandMeta } from "./session";

/** What `shell_command_run` returns. */
export type ShellCommandOutput = {
  stdout: string;
  stderr: string;
  exitCode: number | null;
  truncated: boolean;
  stopped: boolean;
};

/** `!git status` runs `git status`, like Claude Code's bash mode. */
export function consumeShellCommand(text: string): string | null {
  if (!text.startsWith("!")) return null;
  return text.slice(1).trim() || null;
}

export function appendShellCommand(
  session: Session,
  id: string,
  command: string,
  cwd: string,
  startedAt = Date.now(),
): Session {
  return {
    ...session,
    blocks: [
      ...session.blocks,
      {
        id,
        role: "shell",
        text: command,
        startedAt,
        shell: { cwd, status: "running" },
      },
    ],
  };
}

export function finishShellCommand(
  session: Session,
  id: string,
  result: ShellCommandOutput | { error: string },
  endedAt = Date.now(),
): Session {
  return {
    ...session,
    blocks: session.blocks.map((block) => {
      if (block.id !== id || !block.shell) return block;
      const shell: ShellCommandMeta =
        "error" in result
          ? { cwd: block.shell.cwd, status: "failed", stderr: result.error }
          : {
              cwd: block.shell.cwd,
              status: result.stopped ? "stopped" : "done",
              stdout: result.stdout,
              stderr: result.stderr,
              ...(result.exitCode != null ? { exitCode: result.exitCode } : {}),
              ...(result.truncated ? { truncated: true } : {}),
            };
      return {
        ...block,
        shell,
        durationMs: Math.max(0, endedAt - (block.startedAt ?? endedAt)),
      };
    }),
  };
}

export function shellCommandRunning(session: Session): boolean {
  return session.blocks.some(
    (block) => block.role === "shell" && block.shell?.status === "running",
  );
}

/** Commands run since the agent was last sent a message. */
export function unsentShellCommands(blocks: Block[]): Block[] {
  const unsent: Block[] = [];
  for (let index = blocks.length - 1; index >= 0; index -= 1) {
    const block = blocks[index];
    if (block.role === "user" && !block.draft) break;
    if (block.role === "shell" && block.shell?.status !== "running") {
      unsent.unshift(block);
    }
  }
  return unsent;
}

/**
 * The agent didn't see these commands run, so they ride along with the next
 * message, in the tags Claude Code uses for its own `!` commands.
 */
export function withShellCommands(blocks: Block[], prompt: string): string {
  const commands = unsentShellCommands(blocks);
  if (commands.length === 0) return prompt;
  const context = commands.map((block) => {
    const shell = block.shell!;
    const outcome =
      shell.status === "stopped"
        ? "Stopped before it finished."
        : shell.status === "failed"
          ? "Failed to start."
          : shell.exitCode != null
            ? `Exit code ${shell.exitCode}.`
            : "Ended by a signal.";
    return [
      `<bash-input>${block.text}</bash-input>`,
      `<bash-stdout>${shell.stdout ?? ""}</bash-stdout>`,
      `<bash-stderr>${shell.stderr ?? ""}</bash-stderr>`,
      shell.truncated ? `${outcome} Output was truncated.` : outcome,
    ].join("\n");
  });
  return `I ran ${commands.length === 1 ? "this shell command" : "these shell commands"} myself before this message:\n\n${context.join("\n\n")}\n\n${prompt}`;
}

/** Persist the command and its output; one cut short by a quit reads as stopped. */
export function sanitizeShellCommand(
  value: unknown,
  hydrate: boolean,
): ShellCommandMeta | null {
  if (!value || typeof value !== "object") return null;
  const shell = value as Partial<ShellCommandMeta>;
  if (typeof shell.cwd !== "string") return null;
  const status =
    shell.status === "running"
      ? hydrate
        ? "stopped"
        : "running"
      : shell.status === "done" ||
          shell.status === "stopped" ||
          shell.status === "failed"
        ? shell.status
        : null;
  if (!status) return null;
  return {
    cwd: shell.cwd,
    status,
    ...(typeof shell.stdout === "string" ? { stdout: shell.stdout } : {}),
    ...(typeof shell.stderr === "string" ? { stderr: shell.stderr } : {}),
    ...(typeof shell.exitCode === "number" ? { exitCode: shell.exitCode } : {}),
    ...(shell.truncated === true ? { truncated: true } : {}),
  };
}

import type { Block } from "./session";

export type MonoCodeToolCall = {
  action: string;
  label: string;
};

const ACTION_LABELS: Record<string, string> = {
  "models.list": "List models",
  "sessions.list": "List sessions",
  "sessions.read": "Read a session",
  "sessions.send": "Continue a session",
  "sessions.draft": "Save a draft",
  "sessions.start": "Start a session",
  "folders.list": "List folders",
  "folders.move": "Move a session",
  "notes.list": "List notes",
  "notes.read": "Read a note",
};

/** Recognize the actual app CLI command, not a mention of it in prose/output. */
export function monoCodeToolCall(block: Block): MonoCodeToolCall | undefined {
  if (block.role !== "tool" && block.role !== "approval") return undefined;

  for (const candidate of [
    block.text,
    block.tool?.title,
    block.tool?.preview?.title,
  ]) {
    if (!candidate?.trim()) continue;
    const command = candidate
      .trim()
      .replace(/^Run(?:ning)?\s+command:\s*/i, "");
    const match = command.match(
      /^(?:"([^"\n]+)"|'([^'\n]+)'|([^\s]+))\s+app(?:\s+(--help|[a-z]+(?:\.[a-z]+)?))?(?=\s|$)/i,
    );
    if (!match) continue;
    const executable = match[1] ?? match[2] ?? match[3];
    if (!/(?:^|[/\\])monocode(?:\.exe)?$/i.test(executable)) continue;
    const action = match[4]?.toLowerCase() ?? "--help";
    return {
      action,
      label:
        action === "--help"
          ? "View CLI commands"
          : (ACTION_LABELS[action] ?? "Run a MonoCode command"),
    };
  }
  return undefined;
}

/** A group of only MonoCode calls can be named for the app, not the shell. */
export function monoCodeWorkSummary(
  steps: Block[],
  live: boolean,
): string | undefined {
  if (steps.some((block) => block.interjection || block.role === "system")) {
    return undefined;
  }
  const calls = steps.filter(
    (block) => block.role === "tool" || block.role === "approval",
  );
  if (calls.length === 0 || calls.some((block) => !monoCodeToolCall(block))) {
    return undefined;
  }
  return live ? "Using MonoCode" : "Used MonoCode";
}

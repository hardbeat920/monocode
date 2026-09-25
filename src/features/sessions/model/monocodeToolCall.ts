import type { Block } from "./session";

export type MonoCodeToolCall = {
  action: string;
  label: string;
  command: string;
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

/** Conservatively parse one shell invocation; compound commands use the shell row. */
function shellWords(command: string): string[] | undefined {
  const words: string[] = [];
  let word = "";
  let started = false;
  let quote: "'" | '"' | null = null;
  for (let index = 0; index < command.length; index += 1) {
    const char = command[index];
    if (quote === "'") {
      if (char === "'") quote = null;
      else word += char;
      continue;
    }
    if (char === "'" && !quote) {
      quote = "'";
      started = true;
      continue;
    }
    if (char === '"') {
      quote = quote === '"' ? null : '"';
      started = true;
      continue;
    }
    if (char === "\\") {
      const next = command[index + 1];
      if (!next) return undefined;
      // Preserve ordinary path separators, including quoted Windows paths.
      if (
        (quote === '"' && !/[\\"$`]/.test(next)) ||
        (!quote && !/[\s'"\\;&|<>()[\]{}$`#]/.test(next))
      ) {
        word += char;
      } else {
        word += next;
        index += 1;
      }
      started = true;
      continue;
    }
    if (char === "$" || char === "`") return undefined;
    if (!quote && /[\r\n;&|<>()[\]{}#]/.test(char)) return undefined;
    if (!quote && /\s/.test(char)) {
      if (started) words.push(word);
      word = "";
      started = false;
      continue;
    }
    word += char;
    started = true;
  }
  if (quote) return undefined;
  if (started) words.push(word);
  return words;
}

/** Recognize the actual app CLI command, not a mention of it in prose/output. */
export function monoCodeToolCall(block: Block): MonoCodeToolCall | undefined {
  if (block.role !== "tool" && block.role !== "approval") return undefined;
  // A shell preview retains the original command when the display title was
  // simplified. Never accept a shorter title in place of that command.
  const candidate =
    (block.tool?.preview?.kind === "shell"
      ? block.tool.preview.title
      : undefined) ??
    block.tool?.title ??
    block.text;
  const command = candidate?.trim().replace(/^Run(?:ning)?\s+command:\s*/i, "");
  if (!command) return undefined;
  const words = shellWords(command);
  if (!words || !/(?:^|[/\\])monocode(?:\.exe)?$/i.test(words[0] ?? ""))
    return undefined;
  if (words[1] !== "app") return undefined;
  const action = words[2] ?? "--help";
  if (action === "--help" || action === "help" || action === "-h") {
    return words.length > 3
      ? undefined
      : { action: "--help", label: "View CLI commands", command };
  }
  if (!Object.prototype.hasOwnProperty.call(ACTION_LABELS, action))
    return undefined;
  for (let index = 3; index < words.length; index += 2) {
    if (
      !["--json", "--input", "--request-id"].includes(words[index]) ||
      !words[index + 1]
    )
      return undefined;
  }
  return { action, label: ACTION_LABELS[action], command };
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

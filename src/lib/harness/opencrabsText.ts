import {
  killChild,
  resolveOpenCrabsBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "./child";

const TEXT_CHILD_ID = "monocode-opencrabs-text";
const REQUEST_TIMEOUT_MS = 45_000;

/**
 * One-shot text generation through `opencrabs run --quiet --format json`.
 *
 * Unlike the REPL/server harnesses there is nothing to keep warm: each
 * prompt spawns a short-lived child, captures stdout until exit, and reads
 * the structured summary object. `--quiet` (parity branch and later) keeps
 * stdout payload-only; the parser below still scans backwards for the final
 * parseable object with a string `content` field, so progress noise from a
 * binary without the flag degrades gracefully instead of corrupting output.
 * Calls are serialized because each one carries the full headless context.
 */
let turns: Promise<unknown> = Promise.resolve();

export async function runOpenCrabsTextPrompt(input: {
  cwd: string;
  prompt: string;
  timeoutMs?: number;
}): Promise<string> {
  const run = turns.catch(() => undefined).then(() => promptOnce(input));
  turns = run.then(
    () => undefined,
    () => undefined,
  );
  return run;
}

async function promptOnce(input: {
  cwd: string;
  prompt: string;
  timeoutMs?: number;
}): Promise<string> {
  const { path } = await resolveOpenCrabsBinary();
  const timeoutMs = input.timeoutMs ?? REQUEST_TIMEOUT_MS;
  let stdout = "";
  let exited = false;
  let notifyExit: () => void = () => undefined;
  const exitPromise = new Promise<void>((resolve) => {
    notifyExit = resolve;
  });

  watchChild(
    TEXT_CHILD_ID,
    (line) => {
      stdout += `${line}\n`;
    },
    () => {
      exited = true;
      notifyExit();
    },
  );

  const timer = setTimeout(() => {
    if (!exited) void killChild(TEXT_CHILD_ID);
  }, timeoutMs);

  try {
    await spawnChild(
      TEXT_CHILD_ID,
      path,
      ["run", "--quiet", "--format", "json", input.prompt],
      input.cwd,
    );
    await exitPromise;
  } finally {
    clearTimeout(timer);
    unwatchChild(TEXT_CHILD_ID);
    if (!exited) await killChild(TEXT_CHILD_ID).catch(() => undefined);
  }

  const text = parseRunSummary(stdout);
  if (!text) {
    const nonEmpty = stdout.trim().split("\n").filter(Boolean);
    const tail = nonEmpty[nonEmpty.length - 1] ?? "";
    throw new Error(
      tail
        ? `OpenCrabs text generation failed: ${tail.slice(0, 200)}`
        : "OpenCrabs returned empty output.",
    );
  }
  return text;
}

/** Last JSON object on stdout with a string `content` field, if any. */
export function parseRunSummary(stdout: string): string | null {
  const lines = stdout.split("\n");
  for (let i = lines.length - 1; i >= 0; i--) {
    if (!lines[i].startsWith("{")) continue;
    try {
      const parsed: unknown = JSON.parse(lines.slice(i).join("\n"));
      if (
        parsed &&
        typeof parsed === "object" &&
        "content" in parsed &&
        typeof (parsed as { content: unknown }).content === "string"
      ) {
        const content = (parsed as { content: string }).content.trim();
        if (content) return content;
      }
    } catch {
      // Not the summary object; keep scanning upwards.
    }
  }
  return null;
}

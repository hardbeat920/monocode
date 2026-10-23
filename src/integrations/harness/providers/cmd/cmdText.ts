import {
  killChild,
  resolveCmdBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import { buildCmdSpawnArgs, parseCmdLine, strField } from "./cmdProtocol";

const TEXT_CHILD_PREFIX = "monocode-cmd-text-";
let textSeq = 0;
const REQUEST_TIMEOUT_MS = 90_000;

/**
 * One-shot text generation through `cmd -p --no-session`. Unlike Claude's
 * persistent text child, CommandCode's print mode exits on its own, so each
 * call spawns, collects the result line's finalText, and reaps the child.
 */
export async function runCmdTextPrompt(input: {
  cwd: string;
  prompt: string;
  model?: string;
  timeoutMs?: number;
}): Promise<string> {
  const { path } = await resolveCmdBinary();
  const childId = `${TEXT_CHILD_PREFIX}${++textSeq}`;
  const timeoutMs = input.timeoutMs ?? REQUEST_TIMEOUT_MS;
  let output = "";
  let settled = false;

  const done = new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      reject(new Error("CommandCode text generation timed out"));
      void killChild(childId).catch(() => undefined);
    }, timeoutMs);

    watchChild(
      childId,
      (line) => {
        if (settled) return;
        const rec = parseCmdLine(line);
        if (!rec || strField(rec, "type") !== "result") return;
        settled = true;
        clearTimeout(timer);
        if (strField(rec, "subtype") === "error") {
          reject(new Error(strField(rec, "error") || "CommandCode turn failed"));
          return;
        }
        output = strField(rec, "finalText");
        resolve();
      },
      () => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        if (output.trim()) resolve();
        else reject(new Error("CommandCode text generator exited"));
      },
    );
  });

  try {
    await spawnChild(
      childId,
      path,
      buildCmdSpawnArgs({
        text: input.prompt,
        model: input.model,
        runtimeMode: "supervised",
        noSession: true,
      }),
      input.cwd,
      undefined,
      "cmd",
    );
    await done;
  } finally {
    unwatchChild(childId);
    await killChild(childId).catch(() => undefined);
  }

  const trimmed = output.trim();
  if (!trimmed) throw new Error("CommandCode returned empty output.");
  return trimmed;
}

export function warmupCmdText(_cwd: string): Promise<void> {
  // Nothing to warm: every text call is a fresh one-shot child.
  return Promise.resolve();
}

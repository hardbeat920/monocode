import { nativeModelId } from "../../../../features/sessions/model/models";
import {
  killChild,
  resolveMuseBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import {
  MUSE_AUTH_HELP,
  MuseExecFold,
  museAuthError,
  museEffortForModel,
  musePromptParts,
  museSpawnArgs,
  museStartupError,
} from "./museProtocol";
import type {
  ApprovalDecision,
  SendTurnInput,
  SteerTurnInput,
} from "../../core/types";

const PROMPT_TIMEOUT_MS = 30 * 60_000;
const STDERR_TAIL_LINES = 8;
/**
 * `muse exec` releases its server-side session lock only when the process
 * exits, so a follow-up send that spawns before the previous child has died
 * fails with "session is already in use". Hold the turn open after the
 * terminal event until the exit arrives, and kill the child if it lingers.
 */
export const EXIT_GRACE_MS = 10_000;

type Resume = {
  sessionId: string;
  cwd: string;
};

const resumeByThread = new Map<string, Resume>();
const cancelledThreads = new Set<string>();

/**
 * Live Muse adapter. Spawns one headless `muse exec --json` child per turn
 * and folds its MSP event stream into harness events. Resume rides on the
 * stored Muse session id (`--session-id`); there is no persistent child, so
 * stop/forget only kill an in-flight turn and drop resume state.
 */
export async function sendMuseTurn(input: SendTurnInput): Promise<void> {
  // Note: `muse exec` has no plan mode; a plan intent runs as a normal turn
  // and its output returns as chat messages rather than plan blocks.
  if (cancelledThreads.delete(input.sessionId)) return;

  let binary: string;
  try {
    ({ path: binary } = await resolveMuseBinary());
  } catch (error) {
    throw museStartupError(error);
  }
  if (cancelledThreads.delete(input.sessionId)) return;

  const prompt = musePromptParts(input.text, input.attachments, input.cwd);
  const resume = resumeByThread.get(input.sessionId);
  const modelId = nativeModelId(input.model);
  const args = museSpawnArgs({
    cwd: input.cwd,
    prompt: prompt.text,
    images: prompt.images,
    resumeSessionId: resume && resume.cwd === input.cwd ? resume.sessionId : undefined,
    modelId,
    effort: museEffortForModel(input.modelSettings, modelId),
    runtimeMode: input.runtimeMode,
  });

  const fold = new MuseExecFold();
  const stderrTail: string[] = [];
  let bound = false;

  await new Promise<void>((resolve, reject) => {
    let settled = false;
    let exitGrace: ReturnType<typeof setTimeout> | undefined;
    const finish = (error?: Error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (exitGrace) clearTimeout(exitGrace);
      unwatchChild(input.sessionId);
      if (error) reject(error);
      else resolve();
    };
    const timer = setTimeout(() => {
      void killChild(input.sessionId).catch(() => undefined);
      finish(new Error(`Muse turn timed out after ${PROMPT_TIMEOUT_MS / 60_000} minutes`));
    }, PROMPT_TIMEOUT_MS);

    watchChild(
      input.sessionId,
      (line) => {
        // Non-consuming peek: late output after a cancel must not land in the
        // transcript. The exit handler below owns consuming the flag.
        if (cancelledThreads.has(input.sessionId)) return;
        for (const event of fold.pushLine(line)) {
          if (event.type === "message.completed") continue;
          input.onEvent(event);
        }
        if (fold.sessionId && !bound) {
          bound = true;
          resumeByThread.set(input.sessionId, { sessionId: fold.sessionId, cwd: input.cwd });
          input.onEvent({ type: "session.providerBound", providerSessionId: fold.sessionId });
        }
        if (fold.done) {
          input.onEvent({ type: "message.completed" });
          if (fold.failed) {
            finish(new Error(fold.failed));
            return;
          }
          // Wait for the child to die before resolving so the next send
          // cannot reuse the session id while it is still locked.
          exitGrace = setTimeout(() => {
            void killChild(input.sessionId).catch(() => undefined);
            finish();
          }, EXIT_GRACE_MS);
        }
      },
      (code) => {
        if (cancelledThreads.delete(input.sessionId)) {
          finish();
          return;
        }
        if (fold.done) {
          if (fold.failed) finish(new Error(fold.failed));
          else finish();
          return;
        }
        const tail = stderrTail.join("\n").trim();
        if (code !== 0 && code !== null && museAuthError(tail)) {
          finish(new Error(`${tail}\n\n${MUSE_AUTH_HELP}`));
          return;
        }
        finish(
          new Error(
            tail
              ? `Muse turn failed: ${tail}`
              : `Muse exited before finishing (code ${code ?? "unknown"})`,
          ),
        );
      },
      (line) => {
        const text = line.trim();
        if (!text) return;
        stderrTail.push(text);
        if (stderrTail.length > STDERR_TAIL_LINES) stderrTail.shift();
      },
    );

    spawnChild(input.sessionId, binary, args, input.cwd).then(
      () => input.onAccepted?.(),
      (error: unknown) => finish(museStartupError(error)),
    );
  });
}

export async function cancelMuseTurn(sessionId: string): Promise<void> {
  cancelledThreads.add(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

export function respondMuseApproval(
  _sessionId: string,
  _requestId: number,
  _decision: ApprovalDecision,
): void {
  // `muse exec` stdout is one-way JSONL: a blocked approval emits nothing to
  // answer, and stdin is not an approval channel. Spawn flags in
  // `museApprovalArgs` keep the child from waiting on that prompt.
}

export async function steerMuseTurn(_input: SteerTurnInput): Promise<void> {
  throw new Error("Muse does not support steering an in-flight turn");
}

export async function stopMuseSession(sessionId: string): Promise<void> {
  // Clearing here (not just in the exit handler) matters: stop can land while
  // idle with no child running, and a stale flag would swallow the next turn.
  cancelledThreads.delete(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

export async function forgetMuseSession(sessionId: string): Promise<void> {
  resumeByThread.delete(sessionId);
  cancelledThreads.delete(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

export function bindMuseSession(
  threadId: string,
  providerSessionId: string,
  cwd: string,
): void {
  if (!threadId.trim() || !providerSessionId.trim() || !cwd.trim()) return;
  resumeByThread.set(threadId, { sessionId: providerSessionId, cwd });
}


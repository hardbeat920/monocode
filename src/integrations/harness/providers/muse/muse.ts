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
/** Resolves the in-flight turn. `killChild` drops the exit handler first, so
 * cancel/stop/forget must settle the promise themselves. */
const activeTurns = new Map<string, () => Promise<void>>();
const resolvingThreads = new Set<string>();

/**
 * Live Muse adapter. Spawns one headless `muse exec --json` child per turn
 * and folds its MSP event stream into harness events. Resume rides on the
 * stored Muse session id (`--session-id`); there is no persistent child, so
 * stop/forget only kill an in-flight turn and drop resume state.
 */
export async function sendMuseTurn(input: SendTurnInput): Promise<void> {
  // `muse exec` has no native plan mode. Plan intent stays read-only and
  // returns chat messages rather than structured plan blocks.
  if (cancelledThreads.delete(input.sessionId)) return;

  let binary: string;
  resolvingThreads.add(input.sessionId);
  try {
    ({ path: binary } = await resolveMuseBinary());
  } catch (error) {
    if (cancelledThreads.delete(input.sessionId)) return;
    throw museStartupError(error);
  } finally {
    resolvingThreads.delete(input.sessionId);
  }
  if (cancelledThreads.delete(input.sessionId)) return;

  const prompt = musePromptParts(input.text, input.attachments, input.cwd);
  const resume = resumeByThread.get(input.sessionId);
  const modelId = nativeModelId(input.model);
  const args = museSpawnArgs({
    cwd: input.cwd,
    prompt: prompt.text,
    images: prompt.images,
    resumeSessionId:
      resume && resume.cwd === input.cwd ? resume.sessionId : undefined,
    modelId,
    effort: museEffortForModel(input.modelSettings, modelId),
    runtimeMode: input.intent === "plan" ? "supervised" : input.runtimeMode,
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
      activeTurns.delete(input.sessionId);
      cancelledThreads.delete(input.sessionId);
      clearTimeout(timer);
      if (exitGrace) clearTimeout(exitGrace);
      unwatchChild(input.sessionId);
      if (error) reject(error);
      else resolve();
    };
    const timer = setTimeout(() => {
      void killChild(input.sessionId)
        .catch(() => undefined)
        .then(() => {
          finish(
            new Error(
              `Muse turn timed out after ${PROMPT_TIMEOUT_MS / 60_000} minutes`,
            ),
          );
        });
    }, PROMPT_TIMEOUT_MS);

    watchChild(
      input.sessionId,
      (line) => {
        // Non-consuming peek: late output after a cancel must not land in the
        // transcript. The exit handler below owns consuming the flag.
        if (settled || cancelledThreads.has(input.sessionId)) return;
        for (const event of fold.pushLine(line)) {
          if (event.type === "message.completed") continue;
          input.onEvent(event);
        }
        if (fold.sessionId && !bound) {
          bound = true;
          resumeByThread.set(input.sessionId, {
            sessionId: fold.sessionId,
            cwd: input.cwd,
          });
          input.onEvent({
            type: "session.providerBound",
            providerSessionId: fold.sessionId,
          });
        }
        // One terminal settles the turn. Later stdout must not emit
        // `message.completed` again or arm a second kill timer.
        if (fold.done && !exitGrace) {
          input.onEvent({ type: "message.completed" });
          // Wait for the child to die before resolving so the next send
          // cannot reuse the session id while it is still locked. A failed
          // terminal uses the same wait; the exit handler rejects sooner
          // when the process dies first.
          exitGrace = setTimeout(() => {
            void killChild(input.sessionId)
              .catch(() => undefined)
              .then(() => {
                finish(fold.failed ? new Error(fold.failed) : undefined);
              });
          }, EXIT_GRACE_MS);
        }
      },
      (code) => {
        if (settled) return;
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
        if (settled || cancelledThreads.has(input.sessionId)) return;
        const text = line.trim();
        if (!text) return;
        stderrTail.push(text);
        if (stderrTail.length > STDERR_TAIL_LINES) stderrTail.shift();
      },
    );

    let spawnReady: Promise<void>;
    activeTurns.set(input.sessionId, async () => {
      // The backend may not have registered the child yet. Keep this turn
      // pending until spawn completes so the next turn cannot race its kill.
      await killChild(input.sessionId).catch(() => undefined);
      await spawnReady.catch(() => undefined);
      await killChild(input.sessionId).catch(() => undefined);
      finish();
    });
    spawnReady = spawnChild(
      input.sessionId,
      binary,
      args,
      input.cwd,
      undefined,
      "muse",
    );
    void spawnReady.then(
      () => {
        if (settled) {
          void killChild(input.sessionId).catch(() => undefined);
          return;
        }
        if (!cancelledThreads.has(input.sessionId)) input.onAccepted?.();
      },
      (error: unknown) => {
        if (!cancelledThreads.has(input.sessionId))
          finish(museStartupError(error));
      },
    );
  });
}

export async function cancelMuseTurn(sessionId: string): Promise<void> {
  const settle = activeTurns.get(sessionId);
  // Mute output that is already queued. A live turn clears the flag when it
  // settles; a cancel during binary resolve keeps it so send bails out.
  cancelledThreads.add(sessionId);
  if (settle) await settle();
  else await killChild(sessionId).catch(() => undefined);
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
  if (activeTurns.has(sessionId) || resolvingThreads.has(sessionId)) {
    await cancelMuseTurn(sessionId);
  } else {
    cancelledThreads.delete(sessionId);
    await killChild(sessionId).catch(() => undefined);
  }
}

export async function forgetMuseSession(sessionId: string): Promise<void> {
  resumeByThread.delete(sessionId);
  await stopMuseSession(sessionId);
}

export function bindMuseSession(
  threadId: string,
  providerSessionId: string,
  cwd: string,
): void {
  if (!threadId.trim() || !providerSessionId.trim() || !cwd.trim()) return;
  resumeByThread.set(threadId, { sessionId: providerSessionId, cwd });
}

import { nativeModelId } from "../../../../features/sessions/model/models";
import { openCrabsPromptBlocks } from "./opencrabsPrompt";
import type {
  ApprovalDecision,
  CompactContextInput,
  SendTurnInput,
  SteerTurnInput,
} from "../../core/types";
import {
  cancelledThreads,
  CONTROL_TIMEOUT_MS,
  ensureLive,
  liveByThread,
  type Live,
  PROMPT_TIMEOUT_MS,
  SERVER_HELP,
  stopOpenCrabsSession,
} from "./opencrabsLive";

// Lifecycle (spawn, handshake, resume, teardown) lives in opencrabsLive;
// re-exported here so existing import sites keep working.
export {
  bindOpenCrabsSession,
  forgetOpenCrabsSession,
  spawnArgs,
  stopOpenCrabsSession,
} from "./opencrabsLive";

/**
 * Live OpenCrabs adapter turn orchestration. Spawns `opencrabs acp` (via
 * opencrabsLive) and talks Agent Client Protocol over stdio. Permission
 * requests surface in the UI unless the runtime mode auto-answers them.
 */
export async function sendOpenCrabsTurn(input: SendTurnInput): Promise<void> {
  let live: Live;
  try {
    live = await ensureLive(input);
  } catch (error) {
    cancelledThreads.delete(input.sessionId);
    throw error;
  }
  if (cancelledThreads.delete(input.sessionId)) return;

  live.onEvent = input.onEvent;
  live.runtimeMode = input.runtimeMode;
  live.planning = input.intent === "plan";
  live.turns = live.turns
    .catch(() => undefined)
    .then(async () => {
      live.cancelled = false;
      live.muteUpdates = false;
      try {
        await applyModelSelection(live, input);
        await applyRuntimeMode(live, input);
        if (live.cancelled) return;
        await prompt(live, input);
      } catch (error) {
        if (live.cancelled) return;
        throw error;
      }
    });
  try {
    await live.turns;
  } catch (error) {
    // A timed-out or failed turn leaves the child's protocol state unknowable.
    // Keep the provider session id, but recycle the process so the next turn
    // resumes on a fresh transport instead of a wedged one.
    if (liveByThread.get(input.sessionId) === live) {
      await stopOpenCrabsSession(input.sessionId);
    }
    throw error;
  }
}

export async function steerOpenCrabsTurn(input: SteerTurnInput): Promise<void> {
  const live = liveByThread.get(input.sessionId);
  if (!live) throw new Error("No active OpenCrabs session");
  const blocks = await openCrabsPromptBlocks(input.text, input.attachments);
  if (blocks.length === 0) return;
  await live.acp
    // Plain method name, not the "_session/steer" ext-prefix: released
    // opencrabs (v0.5.2) only registers the plain name, and JSON-RPC drops
    // unknown notifications silently — an ext-prefixed notify is a no-op on
    // every released binary. The parity server accepts both spellings.
    .notify("session/steer", {
      sessionId: live.acpSessionId,
      prompt: blocks,
    })
    .catch(() => undefined);
}

export function respondOpenCrabsApproval(
  sessionId: string,
  requestId: number,
  decision: ApprovalDecision,
): void {
  liveByThread.get(sessionId)?.approvals.get(requestId)?.(decision);
}

/** Abort the in-flight prompt without tearing down the ACP session. */
export async function cancelOpenCrabsTurn(sessionId: string): Promise<void> {
  const live = liveByThread.get(sessionId);
  if (!live) {
    cancelledThreads.add(sessionId);
    return;
  }
  live.cancelled = true;
  live.muteUpdates = true;
  for (const [, resolve] of live.approvals) resolve("deny");
  live.approvals.clear();
  await live.acp
    .notify("session/cancel", { sessionId: live.acpSessionId })
    .catch(() => undefined);
  live.acp.rejectPending(new Error("cancelled"));
}

/**
 * Compact the session's context window via `session/compact`. The server
 * runs its native summarization turn; the request resolves when it ends.
 * Compaction of a long conversation is a full turn, so it rides the turn
 * queue and the prompt timeout rather than the control timeout.
 */
export async function compactOpenCrabsContext(
  input: CompactContextInput,
): Promise<void> {
  const live = liveByThread.get(input.sessionId) ??
    (await ensureLive({ ...input, text: "" }));
  live.onEvent = input.onEvent;
  live.turns = live.turns
    .catch(() => undefined)
    .then(async () => {
      if (live.cancelled) return;
      await live.acp.request(
        "session/compact",
        { sessionId: live.acpSessionId },
        PROMPT_TIMEOUT_MS,
      );
    });
  try {
    await live.turns;
  } catch (error) {
    // Same contract as a failed prompt turn: a timed-out compact leaves the
    // child's protocol state unknowable. Keep the ACP session id, recycle
    // the process so the next turn resumes on a fresh transport instead of
    // a wedged one.
    if (liveByThread.get(input.sessionId) === live) {
      await stopOpenCrabsSession(input.sessionId);
    }
    throw error;
  }
}

/**
 * Model selection is best-effort: the static catalog ships only `default`
 * (empty native id, skipped here), while live catalog entries carry
 * `provider/model` pairs the server routes through `session/set_model`.
 */
async function applyModelSelection(
  live: Live,
  input: SendTurnInput,
): Promise<void> {
  const base = nativeModelId(input.model).trim();
  if (!base) return;
  try {
    await live.acp.request(
      "session/set_model",
      { sessionId: live.acpSessionId, modelId: base },
      CONTROL_TIMEOUT_MS,
    );
    // The badge only hears about model switches through configChanged —
    // without it the picker and the turn can quietly disagree.
    live.onEvent({ type: "session.configChanged", model: input.model });
  } catch (error) {
    // The switch failed but the turn proceeds on the previous model — say
    // so instead of letting the picker silently disagree with the turn.
    live.onEvent({
      type: "session.error",
      message: `Model switch to ${base} failed — continuing with the previous model (${String(error)})`,
    });
  }
}

/**
 * Push the runtime/plan mode server-side so the approval policy lives where
 * the tools run. Client-side gating in handlePermission stays as backstop,
 * and an older binary without set_mode support degrades to it.
 */
async function applyRuntimeMode(
  live: Live,
  input: SendTurnInput,
): Promise<void> {
  const modeId = input.intent === "plan" ? "plan" : input.runtimeMode;
  await live.acp
    .request(
      "session/set_mode",
      { sessionId: live.acpSessionId, modeId },
      CONTROL_TIMEOUT_MS,
    )
    .catch((error: unknown) => {
      const detail = error instanceof Error ? error.message : String(error);
      console.debug("[monocode] opencrabs set_mode failed", detail);
      if (/timed out|not running|exited|closed|pipe/i.test(detail)) throw error;
      // Old binaries without set_mode answer "method not found" — that is
      // the designed degradation to client-side gating, not a failure worth
      // a transcript line. Anything else the user should hear: the next
      // turn then runs a different approval policy than the chip promises.
      if (!/method not found/i.test(detail)) {
        live.onEvent({
          type: "session.error",
          message: `Mode switch to ${modeId} failed — the next turn keeps the previous tool-approval policy (${detail})`,
        });
      }
    });
}

async function prompt(live: Live, input: SendTurnInput): Promise<void> {
  try {
    const blocks = await openCrabsPromptBlocks(input.text, input.attachments);
    if (blocks.length === 0) return;
    await live.acp.request(
      "session/prompt",
      {
        sessionId: live.acpSessionId,
        prompt: blocks,
      },
      PROMPT_TIMEOUT_MS,
    );
    if (live.cancelled) return;
    live.onEvent({ type: "message.completed" });
    live.onEvent({ type: "reasoning.completed" });
  } catch (error) {
    if (live.cancelled) return;
    const detail = error instanceof Error ? error.message : String(error);
    live.onEvent({
      type: "session.error",
      message: /timed out|not running|exited|closed|pipe|method not found/i.test(
        detail,
      )
        ? `${detail.trim()}\n\n${SERVER_HELP}`
        : detail,
    });
    throw error;
  }
}

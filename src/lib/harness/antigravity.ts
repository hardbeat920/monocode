import { nativeModelId } from "../models";
import type { RuntimeMode } from "../session";
import {
  killChild,
  resolveAntigravityBinary,
  spawnChild,
  unwatchChild,
  watchChild,
  writeChild,
} from "./child";
import {
  buildAntigravitySpawnArgs,
  buildAntigravityUserMessage,
  mapAntigravityEvents,
  parseAntigravityLine,
} from "./antigravityProtocol";
import type {
  ApprovalDecision,
  HarnessEvent,
  SendTurnInput,
  SteerTurnInput,
} from "./types";

type Live = {
  sessionId: string;
  cwd: string;
  model: string;
  settingsKey: string;
  runtimeMode: RuntimeMode;
  conversationId?: string;
  turnIndex: number;
  hadDeltas: boolean;
  onEvent: (event: HarnessEvent) => void;
  turnDone: (() => void) | null;
  turnFailed: ((error: Error) => void) | null;
  turns: Promise<void>;
};

type Resume = {
  conversationId?: string;
  cwd: string;
};

const liveByThread = new Map<string, Live>();
const resumeByThread = new Map<string, Resume>();
const cancelledThreads = new Set<string>();

let resolveAntigravityBinaryImpl: () => Promise<{ path: string }> = resolveAntigravityBinary;

/** Test seam. */
export function setAntigravityBinaryResolver(
  fn: () => Promise<{ path: string }>,
): void {
  resolveAntigravityBinaryImpl = fn;
}

function computeSettingsKey(input: SendTurnInput): string {
  return `${input.model}:${input.runtimeMode}:${input.modelSettings?.effort ?? ""}`;
}

export async function sendAntigravityTurn(input: SendTurnInput): Promise<void> {
  let live: Live;
  try {
    live = await ensureLive(input);
  } catch (error) {
    cancelledThreads.delete(input.sessionId);
    throw error;
  }
  if (cancelledThreads.delete(input.sessionId)) {
    await stopAntigravitySession(input.sessionId);
    return;
  }

  live.onEvent = input.onEvent;
  live.runtimeMode = input.runtimeMode;
  live.turns = live.turns.catch(() => undefined).then(async () => {
    try {
      await runTurn(live, input);
    } catch (error) {
      throw error;
    }
  });
  await live.turns;
}

export async function steerAntigravityTurn(input: SteerTurnInput): Promise<void> {
  const live = liveByThread.get(input.sessionId);
  if (!live) throw new Error("No active Antigravity session");
  throw new Error("Antigravity does not support steering while a turn is in flight");
}

export function respondAntigravityApproval(
  _sessionId: string,
  _requestId: number,
  _decision: ApprovalDecision,
): void {
  // Antigravity stream-json runs in non-interactive auto/accept-edits mode.
}

export async function cancelAntigravityTurn(sessionId: string): Promise<void> {
  cancelledThreads.add(sessionId);
  const live = liveByThread.get(sessionId);
  if (live) {
    live.turnDone?.();
    live.turnDone = null;
    live.turnFailed = null;
    await killChild(sessionId).catch(() => undefined);
    liveByThread.delete(sessionId);
  }
}

export async function stopAntigravitySession(sessionId: string): Promise<void> {
  cancelledThreads.delete(sessionId);
  const live = liveByThread.get(sessionId);
  liveByThread.delete(sessionId);
  if (live) {
    live.turnDone?.();
    live.turnDone = null;
    live.turnFailed = null;
  }
  unwatchChild(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

export async function forgetAntigravitySession(sessionId: string): Promise<void> {
  cancelledThreads.delete(sessionId);
  resumeByThread.delete(sessionId);
  await stopAntigravitySession(sessionId);
}

export function bindAntigravitySession(
  threadId: string,
  providerSessionId: string,
  cwd: string,
): void {
  const trimmed = providerSessionId.trim();
  if (!trimmed) return;
  resumeByThread.set(threadId, {
    conversationId: trimmed,
    cwd,
  });
}

async function ensureLive(input: SendTurnInput): Promise<Live> {
  const settingsKey = computeSettingsKey(input);
  const existing = liveByThread.get(input.sessionId);
  if (existing) {
    if (existing.cwd === input.cwd && existing.settingsKey === settingsKey) {
      return existing;
    }
    await stopAntigravitySession(input.sessionId);
  }

  // If working directory changed, conversation IDs are workspace-scoped and must not leak
  const resume = resumeByThread.get(input.sessionId);
  if (resume && resume.cwd !== input.cwd) {
    resumeByThread.delete(input.sessionId);
  }
  const activeResume = resumeByThread.get(input.sessionId);

  const { path } = await resolveAntigravityBinaryImpl();
  const nativeId = nativeModelId(input.model);

  const liveRef: { current: Live | null } = { current: null };
  const live: Live = {
    sessionId: input.sessionId,
    cwd: input.cwd,
    model: input.model,
    settingsKey,
    runtimeMode: input.runtimeMode,
    conversationId: activeResume?.conversationId,
    turnIndex: 0,
    hadDeltas: false,
    onEvent: input.onEvent,
    turnDone: null,
    turnFailed: null,
    turns: Promise.resolve(),
  };
  liveRef.current = live;

  watchChild(
    input.sessionId,
    (line) => {
      const current = liveRef.current;
      if (!current) return;
      handleLine(current, line);
    },
    (code) => {
      liveByThread.delete(input.sessionId);
      const current = liveRef.current;
      (current?.onEvent ?? input.onEvent)({ type: "session.ended", code });
      const exitMsg =
        code == null
          ? "Antigravity process terminated"
          : `Antigravity exited with code ${code}`;
      current?.turnFailed?.(new Error(exitMsg));
      if (current) {
        current.turnDone = null;
        current.turnFailed = null;
      }
    },
    (stderrLine) => {
      console.debug("[monocode] antigravity stderr:", stderrLine);
    },
  );

  const spawnArgs = buildAntigravitySpawnArgs({
    model: nativeId,
    conversationId: activeResume?.conversationId,
    runtimeMode: input.runtimeMode,
    effort: input.modelSettings?.effort,
  });

  try {
    await spawnChild(input.sessionId, path, spawnArgs, input.cwd);
  } catch (error) {
    unwatchChild(input.sessionId);
    throw error;
  }

  liveByThread.set(input.sessionId, live);
  live.onEvent({ type: "session.started" });
  return live;
}

function handleLine(live: Live, line: string): void {
  const parsed = parseAntigravityLine(line);
  if (!parsed) return;

  if (parsed.type === "init") {
    live.conversationId = parsed.conversationId;
    resumeByThread.set(live.sessionId, {
      conversationId: parsed.conversationId,
      cwd: live.cwd,
    });
  } else if (parsed.type === "result" && parsed.conversationId) {
    live.conversationId = parsed.conversationId;
    resumeByThread.set(live.sessionId, {
      conversationId: parsed.conversationId,
      cwd: live.cwd,
    });
  }

  if (
    parsed.type === "step_update" &&
    parsed.stepType === "agent_response" &&
    parsed.textDelta
  ) {
    live.hadDeltas = true;
  }

  const callIdPrefix = `tool-${live.sessionId}-${live.turnIndex}`;
  const events = mapAntigravityEvents(parsed, callIdPrefix);

  // If deltas weren't streamed but final response was returned, emit message.delta first
  if (parsed.type === "result" && !live.hadDeltas && parsed.response) {
    live.onEvent({ type: "message.delta", text: parsed.response });
  }

  for (const event of events) {
    live.onEvent(event);
  }

  if (parsed.type === "result" || parsed.type === "error") {
    const done = live.turnDone;
    live.turnDone = null;
    live.turnFailed = null;
    done?.();
  }
}

async function runTurn(live: Live, input: SendTurnInput): Promise<void> {
  live.turnIndex += 1;
  live.hadDeltas = false;
  live.onEvent = input.onEvent;

  const completion = new Promise<void>((resolve, reject) => {
    live.turnDone = resolve;
    live.turnFailed = reject;
  });

  const message = buildAntigravityUserMessage({ text: input.text });
  await writeChild(live.sessionId, message);
  await completion;
}

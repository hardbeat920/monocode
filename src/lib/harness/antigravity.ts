import { nativeModelId } from "../models";
import {
  killChild,
  resolveAntigravityBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "./child";
import {
  eventsFromAntigravityLine,
  nativeAntigravityModelId,
} from "./antigravityProtocol";
import type {
  ApprovalDecision,
  HarnessEvent,
  SendTurnInput,
  SteerTurnInput,
} from "./types";

type Live = {
  cwd: string;
  conversationId?: string;
  cancelled: boolean;
  muteUpdates: boolean;
  onEvent: (event: HarnessEvent) => void;
  turns: Promise<void>;
  turnDone: (() => void) | null;
  turnFailed: ((error: Error) => void) | null;
};

type Resume = {
  conversationId: string;
  cwd: string;
};

const liveByThread = new Map<string, Live>();
const resumeByThread = new Map<string, Resume>();
const cancelledThreads = new Set<string>();

let resolveBinaryImpl: () => Promise<{ path: string }> =
  resolveAntigravityBinary;

/** Test seam. */
export function setAntigravityBinaryResolver(
  fn: () => Promise<{ path: string }>,
): void {
  resolveBinaryImpl = fn;
}

export async function sendAntigravityTurn(
  input: SendTurnInput,
): Promise<void> {
  let live: Live;
  try {
    live = await ensureLive(input);
  } catch (error) {
    cancelledThreads.delete(input.sessionId);
    throw error;
  }
  if (cancelledThreads.delete(input.sessionId)) return;

  live.onEvent = input.onEvent;
  live.turns = live.turns
    .catch(() => undefined)
    .then(async () => {
      live.cancelled = false;
      live.muteUpdates = false;
      try {
        await runTurn(live, input);
      } catch (error) {
        if (live.cancelled) return;
        throw error;
      }
    });
  await live.turns;
}

export async function steerAntigravityTurn(
  _input: SteerTurnInput,
): Promise<void> {
  throw new Error("Antigravity does not support steering an in-flight turn");
}

export function respondAntigravityApproval(
  _sessionId: string,
  _requestId: number,
  _decision: ApprovalDecision,
) {}

export async function cancelAntigravityTurn(
  sessionId: string,
): Promise<void> {
  const live = liveByThread.get(sessionId);
  if (!live) {
    cancelledThreads.add(sessionId);
    return;
  }
  live.cancelled = true;
  live.muteUpdates = true;
  live.turnFailed?.(new Error("cancelled"));
  await killChild(sessionId).catch(() => undefined);
}

export async function stopAntigravitySession(
  sessionId: string,
): Promise<void> {
  cancelledThreads.delete(sessionId);
  const live = liveByThread.get(sessionId);
  liveByThread.delete(sessionId);
  if (live) {
    live.muteUpdates = true;
    live.turnDone = null;
    live.turnFailed = null;
  }
  unwatchChild(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

export async function forgetAntigravitySession(
  sessionId: string,
): Promise<void> {
  resumeByThread.delete(sessionId);
  await stopAntigravitySession(sessionId);
}

export function bindAntigravitySession(
  threadId: string,
  conversationId: string,
  cwd: string,
): void {
  const sessionId = conversationId.trim();
  if (!threadId || !sessionId || !cwd.trim()) return;
  resumeByThread.set(threadId, { conversationId: sessionId, cwd });
}

async function ensureLive(input: SendTurnInput): Promise<Live> {
  const existing = liveByThread.get(input.sessionId);
  if (existing && existing.cwd === input.cwd) {
    existing.onEvent = input.onEvent;
    return existing;
  }
  if (existing) {
    resumeByThread.delete(input.sessionId);
    await stopAntigravitySession(input.sessionId);
  }

  const live: Live = {
    cwd: input.cwd,
    conversationId: resumeByThread.get(input.sessionId)?.conversationId,
    cancelled: false,
    muteUpdates: false,
    onEvent: input.onEvent,
    turns: Promise.resolve(),
    turnDone: null,
    turnFailed: null,
  };
  liveByThread.set(input.sessionId, live);
  return live;
}

async function runTurn(live: Live, input: SendTurnInput): Promise<void> {
  unwatchChild(input.sessionId);
  await killChild(input.sessionId).catch(() => undefined);

  const { path } = await resolveBinaryImpl();
  const native = nativeAntigravityModelId(
    nativeModelId(input.model),
    input.modelSettings,
  );
  const args = [
    `--print=${input.text}`,
    "--output-format",
    "stream-json",
    "--model",
    native,
  ];
  if (live.conversationId) {
    args.push("--conversation", live.conversationId);
  }
  if (input.runtimeMode === "full-access") {
    args.push("--dangerously-skip-permissions");
  }
  if (input.intent === "plan") {
    args.push("--mode", "plan");
  }

  const pending = new Promise<void>((resolve, reject) => {
    live.turnDone = resolve;
    live.turnFailed = reject;
  });

  watchChild(
    input.sessionId,
    (line) => handleLine(live, input.sessionId, line),
    (code) => {
      if (live.muteUpdates) {
        live.turnDone?.();
        live.turnDone = null;
        live.turnFailed = null;
        return;
      }
      live.onEvent({ type: "session.ended", code });
      if (code && code !== 0) {
        live.turnFailed?.(new Error("Antigravity CLI exited"));
      } else {
        live.turnDone?.();
      }
      live.turnDone = null;
      live.turnFailed = null;
    },
  );

  live.onEvent({ type: "session.started" });
  await spawnChild(input.sessionId, path, args, input.cwd);
  await pending;
}

function handleLine(live: Live, sessionId: string, line: string): void {
  if (live.muteUpdates) return;
  for (const event of eventsFromAntigravityLine(line)) {
    if (event.type === "session.providerBound") {
      live.conversationId = event.providerSessionId;
      resumeByThread.set(sessionId, {
        conversationId: event.providerSessionId,
        cwd: live.cwd,
      });
    }
    live.onEvent(event);
    if (event.type === "message.completed") {
      live.turnDone?.();
      live.turnDone = null;
      live.turnFailed = null;
    }
    if (event.type === "session.error") {
      live.turnFailed?.(new Error(event.message));
      live.turnDone = null;
      live.turnFailed = null;
    }
  }
}

/** Test seam. */
export function __antigravityTestReset(): void {
  liveByThread.clear();
  resumeByThread.clear();
  cancelledThreads.clear();
}

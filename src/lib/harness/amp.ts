import { nativeModelId } from "../models";
import {
  killChild,
  resolveAmpBinary,
  spawnChild,
  unwatchChild,
  watchChild,
  writeChild,
} from "./child";
import {
  ampModeFromModel,
  asRecord,
  buildAmpSpawnArgs,
  buildAmpSteerMessage,
  buildAmpUserMessage,
  contextFromAssistant,
  errorFromResult,
  errorFromSystem,
  isErrorResult,
  isEndTurn,
  parseJsonLine,
  previewFromTool,
  sessionIdFromInit,
  stringField,
  textBlocksFromAssistant,
  toolKindFromName,
  toolResultsFromUser,
  toolTitle,
  toolUsesFromAssistant,
} from "./ampProtocol";
import { mergeStream } from "./streamText";
import type { ApprovalDecision, HarnessEvent, SendTurnInput, SteerTurnInput } from "./types";

type InFlightTool = {
  id: string;
  name: string;
  input: Record<string, unknown>;
  title: string;
};

type Live = {
  cwd: string;
  ampSessionId: string | null;
  mode: string | undefined;
  fast: boolean;
  onEvent: (event: HarnessEvent) => void;
  cancelled: boolean;
  muteUpdates: boolean;
  turns: Promise<void>;
  turnDone: (() => void) | null;
  turnFailed: ((error: Error) => void) | null;
  activeTurn: boolean;
  toolsById: Map<string, InFlightTool>;
  emittedAssistant: string;
  emittedReasoning: string;
};

type Resume = {
  sessionId: string;
  cwd: string;
};

const liveByThread = new Map<string, Live>();
const resumeByThread = new Map<string, Resume>();
const cancelledThreads = new Set<string>();

let resolveAmpBinaryImpl: () => Promise<{ path: string }> = resolveAmpBinary;

export function setAmpBinaryResolver(
  fn: () => Promise<{ path: string }>,
): void {
  resolveAmpBinaryImpl = fn;
}

export async function sendAmpTurn(input: SendTurnInput): Promise<void> {
  let live: Live;
  try {
    live = await ensureLive(input);
  } catch (error) {
    cancelledThreads.delete(input.sessionId);
    throw error;
  }
  if (cancelledThreads.delete(input.sessionId)) return;

  live.onEvent = input.onEvent;
  live.turns = live.turns.catch(() => undefined).then(async () => {
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

export async function steerAmpTurn(input: SteerTurnInput): Promise<void> {
  const live = liveByThread.get(input.sessionId);
  if (!live?.activeTurn) throw new Error("No active turn to steer");
  await writeJson(input.sessionId, buildAmpSteerMessage(input.text));
}

export function respondAmpApproval(
  _sessionId: string,
  _requestId: number,
  _decision: ApprovalDecision,
): void {
  // amp uses --dangerously-allow-all, no permission requests over the stream
}

export async function cancelAmpTurn(sessionId: string): Promise<void> {
  const live = liveByThread.get(sessionId);
  if (!live) {
    cancelledThreads.add(sessionId);
    return;
  }
  live.cancelled = true;
  live.muteUpdates = true;
  // amp has no interrupt on the stream, killing the process ends the turn.
  // the thread survives on amp's side and resumes with `threads continue`.
  finishActiveTurn(live, [
    { type: "message.completed" },
    { type: "reasoning.completed" },
  ]);
}

export async function stopAmpSession(sessionId: string): Promise<void> {
  cancelledThreads.delete(sessionId);
  const live = liveByThread.get(sessionId);
  liveByThread.delete(sessionId);
  if (live) {
    live.muteUpdates = true;
    live.activeTurn = false;
    live.turnDone?.();
    live.turnDone = null;
    live.turnFailed = null;
  }
  unwatchChild(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

export async function forgetAmpSession(sessionId: string): Promise<void> {
  resumeByThread.delete(sessionId);
  await stopAmpSession(sessionId);
}

export function bindAmpSession(
  threadId: string,
  providerSessionId: string,
  cwd: string,
): void {
  const sessionId = providerSessionId.trim();
  if (!threadId || !sessionId || !cwd.trim()) return;
  resumeByThread.set(threadId, { sessionId, cwd });
}

async function ensureLive(input: SendTurnInput): Promise<Live> {
  const mode = ampModeFromModel(nativeModelId(input.model));
  const fast = input.modelSettings?.fast === "true";

  const existing = liveByThread.get(input.sessionId);
  if (
    existing &&
    existing.cwd === input.cwd &&
    existing.mode === mode &&
    existing.fast === fast
  ) {
    existing.onEvent = input.onEvent;
    return existing;
  }
  if (existing) {
    resumeByThread.delete(input.sessionId);
    await stopAmpSession(input.sessionId);
  }

  const resume = resumeByThread.get(input.sessionId);
  const canResume = resume != null && resume.cwd === input.cwd;
  if (resume && resume.cwd !== input.cwd) {
    resumeByThread.delete(input.sessionId);
  }

  const { path } = await resolveAmpBinaryImpl();
  const liveRef: { current: Live | null } = { current: null };

  const live: Live = {
    cwd: input.cwd,
    ampSessionId: canResume && resume ? resume.sessionId : null,
    mode,
    fast,
    onEvent: input.onEvent,
    cancelled: false,
    muteUpdates: false,
    turns: Promise.resolve(),
    turnDone: null,
    turnFailed: null,
    activeTurn: false,
    toolsById: new Map(),
    emittedAssistant: "",
    emittedReasoning: "",
  };
  liveRef.current = live;

  watchChild(
    input.sessionId,
    (line) => {
      const current = liveRef.current;
      if (!current) return;
      handleLine(input.sessionId, current, line);
    },
    (code) => {
      liveByThread.delete(input.sessionId);
      input.onEvent({ type: "session.ended", code });
      const current = liveRef.current;
      if (current?.activeTurn) {
        // amp has no turn-failure signal over the stream; waku's reference
        // only sends Error + ProcessExited on exit, never TurnFinished{false}.
        // resolve the turn so tool results already received keep their status.
        // mark any pending tools as completed since the edit likely succeeded.
        for (const [, tool] of current.toolsById) {
          current.onEvent({
            type: "tool.updated",
            callId: tool.id,
            title: tool.title,
            kind: toolKindFromName(tool.name),
            status: "completed",
            preview: previewFromTool(tool.name, tool.input),
          });
        }
        finishActiveTurn(current, [
          { type: "message.completed" },
          { type: "reasoning.completed" },
        ]);
        // surface non-zero exits as a soft error without rejecting the turn
        if (code != null && code !== 0) {
          current.onEvent({
            type: "session.error",
            message: `amp exited with code ${code}`,
          });
        }
      }
      if (current) {
        current.turnDone = null;
        current.turnFailed = null;
      }
    },
  );

  const threadId = canResume && resume ? resume.sessionId : undefined;
  await spawnChild(
    input.sessionId,
    path,
    buildAmpSpawnArgs({ mode, fast, threadId }),
    input.cwd,
  );

  liveByThread.set(input.sessionId, live);

  if (threadId) {
    // resuming: amp will emit the session id in the init message
    live.onEvent({
      type: "session.providerBound",
      providerSessionId: threadId,
    });
  }
  live.onEvent({ type: "session.started" });
  return live;
}

async function runTurn(live: Live, input: SendTurnInput): Promise<void> {
  const message = buildAmpUserMessage(input.text, []);
  const content = (message.message as { content: unknown[] }).content;
  if (content.length === 0) return;

  live.emittedAssistant = "";
  live.emittedReasoning = "";
  live.toolsById.clear();

  const turnPromise = new Promise<void>((resolve, reject) => {
    live.turnDone = resolve;
    live.turnFailed = reject;
  });
  live.activeTurn = true;

  try {
    await writeJson(input.sessionId, message);
    await turnPromise;
  } catch (error) {
    if (live.cancelled) return;
    // app.tsx already emits session.error from the sendTurn catch block;
    // emitting here too duplicates the message in the transcript.
    throw error;
  } finally {
    live.turnDone = null;
    live.turnFailed = null;
  }
}

function handleLine(sessionId: string, live: Live, line: string): void {
  const rec = parseJsonLine(line);
  if (!rec) return;

  const type = stringField(rec, "type");
  if (!type) return;

  // capture session id from system init
  const initId = sessionIdFromInit(rec);
  if (initId) {
    live.ampSessionId = initId;
    resumeByThread.set(sessionId, { sessionId: initId, cwd: live.cwd });
    live.onEvent({
      type: "session.providerBound",
      providerSessionId: initId,
    });
    return;
  }

  if (live.muteUpdates) return;

  if (type === "assistant") {
    handleAssistant(live, rec);
    return;
  }
  if (type === "user") {
    handleUser(live, rec);
    return;
  }
  if (type === "result") {
    handleResult(live, rec);
    return;
  }
  if (type === "system") {
    const error = errorFromSystem(rec);
    if (error) live.onEvent({ type: "session.error", message: error });
  }
}

function handleAssistant(live: Live, rec: Record<string, unknown>): void {
  const used = contextFromAssistant(rec);
  if (used !== undefined) live.onEvent({ type: "context", used });

  // text blocks - use mergeStream to handle snapshots
  const snapshot = textBlocksFromAssistant(rec).join("");
  if (snapshot && snapshot !== live.emittedAssistant) {
    const next = mergeStream(live.emittedAssistant, snapshot);
    const delta = next.slice(live.emittedAssistant.length);
    live.emittedAssistant = next;
    if (delta) live.onEvent({ type: "message.delta", text: delta });
  }

  // thinking blocks
  const message = asRecord(rec.message);
  const content = message?.content;
  if (Array.isArray(content)) {
    for (const block of content) {
      const row = asRecord(block);
      if (stringField(row, "type") === "thinking") {
        const text = stringField(row, "thinking");
        if (text && text !== live.emittedReasoning) {
          const next = mergeStream(live.emittedReasoning, text);
          const delta = next.slice(live.emittedReasoning.length);
          live.emittedReasoning = next;
          if (delta) live.onEvent({ type: "reasoning.delta", text: delta });
        }
      }
    }
  }

  // tool_use blocks
  for (const use of toolUsesFromAssistant(rec)) {
    if (live.toolsById.has(use.id)) continue;
    const tool: InFlightTool = {
      id: use.id,
      name: use.name,
      input: use.input,
      title: toolTitle(use.name, use.input),
    };
    live.toolsById.set(use.id, tool);
    live.onEvent({
      type: "tool.started",
      callId: tool.id,
      title: tool.title,
      kind: toolKindFromName(tool.name),
      status: "pending",
      preview: previewFromTool(tool.name, tool.input),
    });
  }

  // turn completion: amp signals with stop_reason: "end_turn"
  if (isEndTurn(rec)) {
    finishActiveTurn(live, [
      { type: "message.completed" },
      { type: "reasoning.completed" },
    ]);
  }
}

function handleUser(live: Live, rec: Record<string, unknown>): void {
  for (const result of toolResultsFromUser(rec)) {
    const tool = live.toolsById.get(result.toolUseId);
    if (!tool) continue;
    // remove from tracking so the exit handler only settles still-pending tools
    live.toolsById.delete(result.toolUseId);
    live.onEvent({
      type: "tool.updated",
      callId: tool.id,
      title: tool.title,
      kind: toolKindFromName(tool.name),
      status: result.isError ? "failed" : "completed",
      detail: result.text || undefined,
      preview: previewFromTool(tool.name, tool.input, result.text),
    });
  }
}

function handleResult(live: Live, rec: Record<string, unknown>): void {
  if (isErrorResult(rec)) {
    live.onEvent({ type: "session.error", message: errorFromResult(rec) });
  }
  // amp may also send a result at the end, settle the turn if still active
  if (live.activeTurn) {
    finishActiveTurn(live, [
      { type: "message.completed" },
      { type: "reasoning.completed" },
    ]);
  }
}

function finishActiveTurn(live: Live, extraEvents: HarnessEvent[] = []): void {
  live.activeTurn = false;
  for (const event of extraEvents) live.onEvent(event);
  const done = live.turnDone;
  live.turnDone = null;
  live.turnFailed = null;
  if (done) done();
}

function writeJson(
  sessionId: string,
  payload: Record<string, unknown>,
): Promise<void> {
  return writeChild(sessionId, JSON.stringify(payload));
}

import { nativeModelId } from "../../../../features/sessions/model/models";
import type {
  Attachment,
  RuntimeMode,
} from "../../../../features/sessions/model/session";
import {
  killChild,
  readHarnessTextFile,
  resolveCmdBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import {
  buildCmdSpawnArgs,
  cmdContentText,
  cmdEvent,
  cmdEventType,
  cmdExitCodeOf,
  cmdExitError,
  cmdFrameType,
  cmdPreviewFromTool,
  cmdRetryDelayMs,
  cmdStatusFromEvent,
  cmdToolTitle,
  CMD_MAX_TURN_ATTEMPTS,
  formatCmdErrorMessage,
  formatCmdInlineAttachment,
  isCmdTransientNetworkError,
  parseCmdLine,
  resultStatusFromCmd,
  sessionIdFromCmdLine,
  splitCmdAttachments,
  strField,
  summarizeCmdTool,
  toolKindFromCmdName,
  toolRefFromCmdEvent,
  truncateCmdDetail,
  type CmdToolRef,
} from "./cmdProtocol";
import type {
  ApprovalDecision,
  HarnessEvent,
  SendTurnInput,
  SteerTurnInput,
} from "../../core/types";

type OpenTool = {
  name: string;
  title: string;
};

type Live = {
  cwd: string;
  cmdSessionId?: string;
  runtimeMode: RuntimeMode;
  onEvent: (event: HarnessEvent) => void;
  openTools: Map<string, OpenTool>;
  cancelled: boolean;
  muteUpdates: boolean;
  turnSettled: boolean;
  turnDone: (() => void) | null;
  turnFailed: ((error: Error) => void) | null;
  stderrTail: string[];
  turns: Promise<void>;
  started: boolean;
  stallNotified: boolean;
  stallNotifyTimer?: ReturnType<typeof setTimeout>;
  stallKillTimer?: ReturnType<typeof setTimeout>;
};

type Resume = {
  sessionId: string;
  cwd: string;
};

/**
 * Watchdog: a wedged `cmd -p` child must not park a turn forever, but silence
 * is ambiguous — a long quiet tool call is legitimate — so the first
 * threshold only surfaces a status. Only silence past the kill threshold is
 * treated as a dead transport: the turn is failed as transient and the retry
 * loop resumes from the provider session's committed transcript.
 */
const STALL_NOTIFY_MS = 10 * 60_000;
const STALL_KILL_MS = 30 * 60_000;
const MAX_TURNS_PER_RUN = 100;

/** Raised when the child went silent past STALL_KILL_MS; the retry loop
 *  treats it like a dropped connection. */
class CmdStallError extends Error {
  constructor() {
    super("CommandCode stopped responding (no output for 30 minutes).");
  }
}

function clearStallWatchdog(live: Live): void {
  clearTimeout(live.stallNotifyTimer);
  clearTimeout(live.stallKillTimer);
  live.stallNotifyTimer = undefined;
  live.stallKillTimer = undefined;
}

function armStallWatchdog(sessionId: string, live: Live): void {
  clearStallWatchdog(live);
  live.stallNotified = false;
  live.stallNotifyTimer = setTimeout(() => {
    live.stallNotifyTimer = undefined;
    if (liveByThread.get(sessionId) !== live || live.turnSettled) return;
    if (live.stallNotified) return;
    live.stallNotified = true;
    live.onEvent({
      type: "status",
      key: "cmd-connection",
      text: "CommandCode has been quiet for 10 minutes — still waiting for output.",
    });
  }, STALL_NOTIFY_MS);
  live.stallKillTimer = setTimeout(() => {
    if (liveByThread.get(sessionId) !== live || live.turnSettled) return;
    clearStallWatchdog(live);
    live.turnFailed?.(new CmdStallError());
    live.turnDone = null;
    live.turnFailed = null;
    void killChild(sessionId).catch(() => undefined);
  }, STALL_KILL_MS);
}

/** Re-armed on every stdout/stderr line: only true silence trips the watchdog. */
function noteStallActivity(sessionId: string, live: Live): void {
  if (live.turnSettled || !live.stallKillTimer) return;
  armStallWatchdog(sessionId, live);
}

const liveByThread = new Map<string, Live>();
const resumeByThread = new Map<string, Resume>();
const cancelledThreads = new Set<string>();

let resolveCmdBinaryImpl: () => Promise<{ path: string }> = resolveCmdBinary;

/** Test seam. */
export function setCmdBinaryResolver(
  fn: () => Promise<{ path: string }>,
): void {
  resolveCmdBinaryImpl = fn;
}

/**
 * Live CommandCode adapter. Each turn spawns one `cmd -p` child and streams
 * its NDJSON frames until the final result line; follow-ups resume via
 * `--resume <providerSessionId>`.
 */
export async function sendCmdTurn(input: SendTurnInput): Promise<void> {
  const live = ensureCmdLive(input);
  if (cancelledThreads.delete(input.sessionId)) return;
  live.turns = live.turns.catch(() => undefined).then(() => runCmdTurn(live, input));
  await live.turns;
}

export async function steerCmdTurn(_input: SteerTurnInput): Promise<void> {
  throw new Error("CommandCode does not support steering an in-flight turn");
}

/** `--yolo` pre-authorizes; headless `cmd` cannot answer approvals. */
export function respondCmdApproval(
  _sessionId: string,
  _requestId: number,
  _decision: ApprovalDecision,
): void {}

export async function cancelCmdTurn(sessionId: string): Promise<void> {
  const live = liveByThread.get(sessionId);
  if (!live) {
    cancelledThreads.add(sessionId);
    return;
  }
  live.cancelled = true;
  live.muteUpdates = true;
  clearStallWatchdog(live);
  await killChild(sessionId).catch(() => undefined);
  settleTurn(live);
}

export async function stopCmdSession(sessionId: string): Promise<void> {
  cancelledThreads.delete(sessionId);
  const live = liveByThread.get(sessionId);
  if (live) {
    live.cancelled = true;
    live.muteUpdates = true;
    clearStallWatchdog(live);
    settleTurn(live);
  }
  liveByThread.delete(sessionId);
  unwatchChild(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

export async function forgetCmdSession(sessionId: string): Promise<void> {
  resumeByThread.delete(sessionId);
  await stopCmdSession(sessionId);
}

export function bindCmdSession(
  threadId: string,
  providerSessionId: string,
  cwd: string,
): void {
  const sessionId = providerSessionId.trim();
  if (!threadId || !sessionId || !cwd.trim()) return;
  resumeByThread.set(threadId, { sessionId, cwd });
}

function ensureCmdLive(input: SendTurnInput): Live {
  const existing = liveByThread.get(input.sessionId);
  if (existing && existing.cwd === input.cwd) {
    existing.onEvent = input.onEvent;
    existing.runtimeMode = input.runtimeMode;
    return existing;
  }
  if (existing) {
    resumeByThread.delete(input.sessionId);
    void stopCmdSession(input.sessionId);
  }
  const resume = resumeByThread.get(input.sessionId);
  const canResume = resume != null && resume.cwd === input.cwd;
  if (resume && resume.cwd !== input.cwd) {
    resumeByThread.delete(input.sessionId);
  }
  const live: Live = {
    cwd: input.cwd,
    cmdSessionId: canResume && resume ? resume.sessionId : undefined,
    runtimeMode: input.runtimeMode,
    onEvent: input.onEvent,
    openTools: new Map(),
    cancelled: false,
    muteUpdates: false,
    turnSettled: false,
    turnDone: null,
    turnFailed: null,
    stderrTail: [],
    turns: Promise.resolve(),
    started: false,
    stallNotified: false,
  };
  liveByThread.set(input.sessionId, live);
  return live;
}

type PreparedCmdPrompt = { text: string; attachments?: Attachment[] };

/**
 * Print mode passes the prompt as one argv value. Windows caps the whole
 * command line at 32k characters, so keep inline attachment content under a
 * conservative budget and leave the rest as path references.
 */
const CMD_INLINE_PROMPT_BUDGET = 24_000;

/**
 * Text attachments (markdown, docs, code) are inlined into the prompt so the
 * `cmd` CLI never forwards them to its backend as file parts (which rejects
 * types like text/markdown). Unreadable files fall back to path references.
 */
async function prepareCmdPrompt(
  text: string,
  attachments?: Attachment[],
): Promise<PreparedCmdPrompt> {
  if (!attachments || attachments.length === 0) return { text, attachments };
  const { inline, referenced } = splitCmdAttachments(attachments);
  if (inline.length === 0) return { text, attachments };
  const blocks: string[] = [];
  const failed: Attachment[] = [];
  let used = text.length;
  for (const attachment of inline) {
    try {
      const content = await readHarnessTextFile(attachment.path ?? "");
      const block = formatCmdInlineAttachment(
        attachment.name,
        attachment.mimeType,
        content,
      );
      if (used + block.length > CMD_INLINE_PROMPT_BUDGET) {
        failed.push(attachment);
        continue;
      }
      used += block.length;
      blocks.push(block);
    } catch {
      // Fall through: the path reference still gives the agent a way
      // to read the file with its own tools.
      failed.push(attachment);
    }
  }
  const inlined = blocks.join("\n\n");
  return {
    text: inlined && text.trim() ? `${text}\n\n${inlined}` : `${text}${inlined}`,
    attachments: [...referenced, ...failed],
  };
}

/**
 * The CLI commits the user message to its session before the first model
 * call, so after a connection failure the transcript already holds the
 * request (and any completed tool results). Respawning with the original
 * prompt would duplicate it; a continuation nudge into the resumed session is
 * the equivalent of the CLI's own in-process stream restart.
 */
const CMD_RETRY_CONTINUATION_PROMPT =
  "Your previous response was interrupted by a connection problem. " +
  "Continue exactly where you left off: do not repeat completed work, " +
  "re-run tools whose results you already have, or restate output that was already produced.";

async function runCmdTurn(live: Live, input: SendTurnInput): Promise<void> {
  const prepared = await prepareCmdPrompt(input.text, input.attachments);
  let attempt = 0;
  while (true) {
    attempt++;
    try {
      await runCmdTurnOnce(live, input, prepared, attempt > 1);
      return;
    } catch (error) {
      if (live.cancelled) return;
      const rawMessage = error instanceof Error ? error.message : String(error);
      const isTransient =
        error instanceof CmdStallError ||
        isCmdTransientNetworkError(rawMessage, cmdExitCodeOf(error));
      const canRetry = isTransient && attempt < CMD_MAX_TURN_ATTEMPTS;

      if (canRetry) {
        const delayMs = cmdRetryDelayMs(attempt - 1);
        live.onEvent({
          type: "status",
          key: "cmd-connection",
          text:
            `Connection issue. Restarting the turn in ${Math.round(delayMs / 1000)}s ` +
            `(attempt ${attempt + 1} of ${CMD_MAX_TURN_ATTEMPTS})…`,
        });
        await new Promise((resolve) => setTimeout(resolve, delayMs));
        if (live.cancelled) return;
        continue;
      }

      const formatted = formatCmdErrorMessage(rawMessage);
      live.onEvent({ type: "session.error", message: formatted });
      throw new Error(formatted);
    }
  }
}

async function runCmdTurnOnce(
  live: Live,
  input: SendTurnInput,
  prepared: PreparedCmdPrompt,
  isRetry: boolean,
): Promise<void> {
  const { path } = await resolveCmdBinaryImpl();
  live.cancelled = false;
  live.muteUpdates = false;
  live.turnSettled = false;
  live.openTools.clear();
  live.stderrTail = [];

  if (!live.started) {
    live.started = true;
    live.onEvent({ type: "session.started" });
  }

  const resumeId = live.cmdSessionId;
  const continuation = isRetry && Boolean(resumeId);
  const args = buildCmdSpawnArgs({
    text: continuation ? CMD_RETRY_CONTINUATION_PROMPT : prepared.text,
    attachments: continuation ? undefined : prepared.attachments,
    model: nativeModelId(input.model),
    effort: input.modelSettings?.effort,
    resume: resumeId,
    maxTurns: MAX_TURNS_PER_RUN,
    runtimeMode: input.runtimeMode,
    plan: input.intent === "plan",
  });

  const turnPromise = new Promise<void>((resolve, reject) => {
    live.turnDone = resolve;
    live.turnFailed = reject;
  });

  watchChild(
    input.sessionId,
    (line) => {
      noteStallActivity(input.sessionId, live);
      handleLine(input.sessionId, live, line);
    },
    (code) => {
      // Clean exit after the result line is the normal one-shot ending.
      if (live.turnSettled || liveByThread.get(input.sessionId) !== live) return;
      if (live.cancelled) {
        settleTurn(live);
        return;
      }
      const error = cmdExitError(code, live.stderrTail.join("\n"));
      live.turnFailed?.(error);
      live.turnDone = null;
      live.turnFailed = null;
    },
    (line) => {
      noteStallActivity(input.sessionId, live);
      live.stderrTail.push(line);
      if (live.stderrTail.length > 10) live.stderrTail.shift();
    },
  );
  armStallWatchdog(input.sessionId, live);

  try {
    // The provider tag is required: the backend only spawns resolved
    // harness CLIs (provider-less spawns are rejected).
    await spawnChild(input.sessionId, path, args, input.cwd, undefined, "cmd");
    await turnPromise;
  } catch (error) {
    if (live.cancelled) return;
    throw error;
  } finally {
    clearStallWatchdog(live);
    live.turnDone = null;
    live.turnFailed = null;
    unwatchChild(input.sessionId);
    await killChild(input.sessionId).catch(() => undefined);
  }
}

function handleLine(sessionId: string, live: Live, line: string): void {
  const rec = parseCmdLine(line);
  if (!rec || live.muteUpdates) return;

  if (cmdFrameType(rec) === "result") {
    handleResult(sessionId, live, rec);
    return;
  }
  if (cmdFrameType(rec) !== "event") return;

  const event = cmdEvent(rec);
  if (!event) return;
  bindProviderSession(sessionId, live, rec);

  const recovery = cmdStatusFromEvent(event);
  if (recovery) {
    live.onEvent({ type: "status", key: recovery.key, text: recovery.text });
    return;
  }

  switch (cmdEventType(rec)) {
    case "text_delta": {
      const delta = strField(event, "delta");
      if (delta) live.onEvent({ type: "message.delta", text: delta });
      return;
    }
    case "tool_queued": {
      const ref = toolRefFromCmdEvent(event);
      if (!ref) return;
      const title = cmdToolTitle(ref.name, ref.input);
      live.openTools.set(ref.callId, { name: ref.name, title });
      live.onEvent({
        type: "tool.started",
        callId: ref.callId,
        title,
        kind: toolKindFromCmdName(ref.name),
        status: "pending",
        preview: cmdPreviewFromTool(ref.name, ref.input),
      });
      return;
    }
    case "tool_running": {
      const ref = toolRefFromCmdEvent(event);
      if (!ref) return;
      const open = live.openTools.get(ref.callId);
      live.onEvent({
        type: "tool.updated",
        callId: ref.callId,
        title: open?.title ?? cmdToolTitle(ref.name, ref.input),
        kind: toolKindFromCmdName(ref.name),
        status: "in_progress",
        preview: cmdPreviewFromTool(ref.name, ref.input),
      });
      return;
    }
    case "tool_update": {
      const ref = toolRefFromCmdEvent(event);
      if (!ref) return;
      const open = live.openTools.get(ref.callId);
      const detail = truncateCmdDetail(cmdContentText(event.partial));
      if (!detail) return;
      live.onEvent({
        type: "tool.updated",
        callId: ref.callId,
        title: open?.title ?? cmdToolTitle(ref.name, ref.input),
        kind: toolKindFromCmdName(ref.name),
        status: "in_progress",
        detail,
        preview: cmdPreviewFromTool(ref.name, ref.input),
      });
      return;
    }
    case "tool_completed": {
      const ref = toolRefFromCmdEvent(event);
      if (!ref) return;
      const result = truncateCmdDetail(cmdContentText(event.result));
      finishOpenTool(live, ref, "completed", result || undefined);
      return;
    }
    case "tool_hook_blocked": {
      const ref = toolRefFromCmdEvent(event);
      if (!ref) return;
      const detail =
        strField(event, "hookOutput") ||
        "CommandCode blocked this tool in headless mode.";
      finishOpenTool(live, ref, "denied", detail);
      return;
    }
    default:
      return;
  }
}

function finishOpenTool(
  live: Live,
  ref: CmdToolRef,
  status: string,
  detail?: string,
): void {
  const open = live.openTools.get(ref.callId);
  live.openTools.delete(ref.callId);
  live.onEvent({
    type: "tool.updated",
    callId: ref.callId,
    title: open?.title ?? cmdToolTitle(ref.name, ref.input),
    kind: toolKindFromCmdName(ref.name),
    status,
    detail: detail ?? summarizeCmdTool(ref.name, ref.input),
    preview: cmdPreviewFromTool(ref.name, ref.input),
  });
}

function bindProviderSession(
  sessionId: string,
  live: Live,
  rec: Record<string, unknown>,
): void {
  const providerId = sessionIdFromCmdLine(rec);
  if (!providerId || providerId === live.cmdSessionId) return;
  live.cmdSessionId = providerId;
  resumeByThread.set(sessionId, { sessionId: providerId, cwd: live.cwd });
  live.onEvent({ type: "session.providerBound", providerSessionId: providerId });
}

function handleResult(
  sessionId: string,
  live: Live,
  rec: Record<string, unknown>,
): void {
  bindProviderSession(sessionId, live, rec);
  const outcome = resultStatusFromCmd(rec);
  if (outcome.status === "failed") {
    live.turnFailed?.(new Error(outcome.error ?? "CommandCode turn failed"));
    live.turnDone = null;
    live.turnFailed = null;
    return;
  }
  for (const [callId, open] of live.openTools) {
    live.openTools.delete(callId);
    live.onEvent({
      type: "tool.updated",
      callId,
      title: open.title,
      kind: toolKindFromCmdName(open.name),
      status: "completed",
    });
  }
  if (outcome.maxTurns) {
    live.onEvent({ type: "status", text: "Hit the turn limit — partial results." });
  }
  settleTurn(live);
}

/** Resolve the turn promise once; the child exit that follows is expected. */
function settleTurn(live: Live): void {
  if (live.turnSettled) return;
  live.turnSettled = true;
  live.onEvent({ type: "message.completed" });
  live.onEvent({ type: "reasoning.completed" });
  live.turnDone?.();
  live.turnDone = null;
  live.turnFailed = null;
}

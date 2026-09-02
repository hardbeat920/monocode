import { invoke } from "@tauri-apps/api/core";
import { modelContextWindow } from "../models";
import type { Attachment } from "../session";
import {
  killChild,
  resolveAntigravityBinary,
  spawnChild,
  unwatchChild,
  watchChild,
  writeChild,
} from "./child";
import {
  antigravityContextWindow,
  attachmentDirs,
  buildAntigravitySpawnArgs,
  buildAntigravityUserMessage,
  effectiveAntigravitySettings,
  mapAntigravityLine,
  parseAntigravityLine,
} from "./antigravityProtocol";
import type {
  ApprovalDecision,
  HarnessEvent,
  SendTurnInput,
  SteerTurnInput,
} from "./types";

type Live = {
  cwd: string;
  settingsKey: string;
  conversationId: string;
  contextWindow?: number;
  onEvent: (event: HarnessEvent) => void;
  turns: Promise<void>;
  initialized: boolean;
  initDone: (() => void) | null;
  initFailed: ((error: Error) => void) | null;
  turnDone: (() => void) | null;
  turnFailed: ((error: Error) => void) | null;
  activeTurn: boolean;
  cancelled: boolean;
  muteUpdates: boolean;
  emittedAssistant: string;
  emittedThinking: string;
  exitError: Error | null;
  addedDirs: string[];
};

import {
  type UserQuestion,
  type UserQuestionReply,
  selectedAnswerLabels,
} from "../userQuestion";

type Resume = { conversationId: string; cwd: string };

const INIT_TIMEOUT_MS = 90_000;
const liveByThread = new Map<string, Live>();
const resumeByThread = new Map<string, Resume>();
const cancelledThreads = new Set<string>();
const stagedAttachmentPaths = new Map<string, string>();
const pendingQuestionsByThread = new Map<string, Map<number, UserQuestion[]>>();

let resolveAntigravityBinaryImpl: () => Promise<{ path: string }> =
  resolveAntigravityBinary;

/** Test seam. */
export function setAntigravityBinaryResolver(
  fn: () => Promise<{ path: string }>,
): void {
  resolveAntigravityBinaryImpl = fn;
}

export async function sendAntigravityTurn(input: SendTurnInput): Promise<void> {
  // stream-json only accepts text. Browser-pasted images are base64-only until
  // we materialize them; file references then make them available to agy.
  const attachments = await materializeAttachments(
    input.sessionId,
    input.attachments,
  );
  const prepared =
    attachments === input.attachments ? input : { ...input, attachments };
  let live: Live;
  try {
    live = await ensureLive(prepared);
  } catch (error) {
    cancelledThreads.delete(input.sessionId);
    throw error;
  }
  if (cancelledThreads.delete(input.sessionId)) return;

  live.onEvent = prepared.onEvent;
  live.turns = live.turns
    .catch(() => undefined)
    .then(async () => {
      live.cancelled = false;
      live.muteUpdates = false;
      await runTurn(live, prepared);
    });
  try {
    await live.turns;
  } catch (error) {
    if (liveByThread.get(input.sessionId) === live) {
      await stopAntigravitySession(input.sessionId);
    }
    throw error;
  }
}

export async function steerAntigravityTurn(input: SteerTurnInput): Promise<void> {
  const live = liveByThread.get(input.sessionId);
  if (!live?.activeTurn) throw new Error("No active turn to steer");

  const attachments = await materializeAttachments(
    input.sessionId,
    input.attachments,
  );
  const message = buildAntigravityUserMessage({
    text: input.text,
    cwd: live.cwd,
    attachments,
  });
  if (!message) return;

  await writeChild(input.sessionId, JSON.stringify(message));
}

/** Headless stream-json has no approval response channel. */
export function respondAntigravityApproval(
  _sessionId: string,
  _requestId: number,
  _decision: ApprovalDecision,
): void {}

export function respondAntigravityQuestion(
  sessionId: string,
  requestId: number,
  reply: UserQuestionReply,
): void {
  const live = liveByThread.get(sessionId);
  if (!live) return;

  const questions = pendingQuestionsByThread.get(sessionId)?.get(requestId);
  pendingQuestionsByThread.get(sessionId)?.delete(requestId);

  live.onEvent({
    type: "question.resolved",
    requestId,
    decision: reply.kind === "skipped" ? "skipped" : "answered",
  });

  if (reply.kind === "skipped") {
    const message = buildAntigravityUserMessage({
      text: "The user skipped answering the clarifying question(s). Please proceed using your best judgment.",
      cwd: live.cwd,
    });
    if (message) {
      void writeChild(sessionId, JSON.stringify(message)).catch(() => undefined);
    }
    return;
  }

  const answerLines: string[] = [];
  for (const q of questions ?? []) {
    const labels = selectedAnswerLabels(q, reply);
    if (labels.length > 0) {
      answerLines.push(`- ${q.prompt}: ${labels.join(", ")}`);
    }
  }

  const answerText =
    answerLines.length > 0
      ? `User response to clarifying questions:\n${answerLines.join("\n")}`
      : "User answered clarifying question.";

  const message = buildAntigravityUserMessage({
    text: answerText,
    cwd: live.cwd,
  });
  if (message) {
    void writeChild(sessionId, JSON.stringify(message)).catch(() => undefined);
  }
}

export async function cancelAntigravityTurn(sessionId: string): Promise<void> {
  const live = liveByThread.get(sessionId);
  if (!live) {
    cancelledThreads.add(sessionId);
    return;
  }
  live.cancelled = true;
  live.muteUpdates = true;
  live.activeTurn = false;
  live.onEvent({ type: "message.completed" });
  live.onEvent({ type: "reasoning.completed" });
  live.turnDone?.();
  live.turnDone = null;
  live.turnFailed = null;
  // Closing stdin waits for the active turn. Kill immediately and preserve the
  // conversation id so the next send can resume it in a fresh process.
  liveByThread.delete(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

export async function stopAntigravitySession(sessionId: string): Promise<void> {
  cancelledThreads.delete(sessionId);
  pendingQuestionsByThread.delete(sessionId);
  const live = liveByThread.get(sessionId);
  liveByThread.delete(sessionId);
  if (live) {
    live.muteUpdates = true;
    live.activeTurn = false;
    live.turnDone?.();
    live.initDone?.();
    live.turnDone = null;
    live.turnFailed = null;
    live.initDone = null;
    live.initFailed = null;
  }
  unwatchChild(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

export async function forgetAntigravitySession(
  sessionId: string,
): Promise<void> {
  resumeByThread.delete(sessionId);
  for (const key of stagedAttachmentPaths.keys()) {
    if (key.startsWith(`${sessionId}:`)) stagedAttachmentPaths.delete(key);
  }
  await stopAntigravitySession(sessionId);
}

export function bindAntigravitySession(
  threadId: string,
  providerSessionId: string,
  cwd: string,
): void {
  const conversationId = providerSessionId.trim();
  if (!threadId || !conversationId || !cwd.trim()) return;
  resumeByThread.set(threadId, { conversationId, cwd });
}

async function ensureLive(input: SendTurnInput): Promise<Live> {
  const settingsKey = settingsKeyFor(input);
  const existing = liveByThread.get(input.sessionId);
  if (
    existing &&
    existing.cwd === input.cwd &&
    existing.settingsKey === settingsKey
  ) {
    const currentAttachmentDirs = attachmentDirs(input.attachments);
    const hasUnaddedDir = currentAttachmentDirs.some(
      (dir) => !existing.addedDirs.includes(dir),
    );
    if (!hasUnaddedDir) {
      existing.onEvent = input.onEvent;
      return existing;
    }
  }
  const priorDirs = existing?.addedDirs ?? [];
  if (existing) await stopAntigravitySession(input.sessionId);

  const saved = resumeByThread.get(input.sessionId);
  const resume = saved?.cwd === input.cwd ? saved.conversationId : undefined;
  if (saved && !resume) resumeByThread.delete(input.sessionId);
  const { path } = await resolveAntigravityBinaryImpl();
  const contextWindow =
    modelContextWindow(input.model) ?? antigravityContextWindow(input.model);
  const liveRef: { current: Live | null } = { current: null };
  const allDirs = [
    ...new Set([...priorDirs, ...attachmentDirs(input.attachments)]),
  ];
  const live: Live = {
    cwd: input.cwd,
    settingsKey,
    conversationId: resume ?? "",
    contextWindow,
    onEvent: input.onEvent,
    turns: Promise.resolve(),
    initialized: false,
    initDone: null,
    initFailed: null,
    turnDone: null,
    turnFailed: null,
    activeTurn: false,
    cancelled: false,
    muteUpdates: false,
    emittedAssistant: "",
    emittedThinking: "",
    exitError: null,
    addedDirs: allDirs,
  };
  liveRef.current = live;

  watchChild(
    input.sessionId,
    (line) => {
      const current = liveRef.current;
      if (current) handleLine(input.sessionId, current, line);
    },
    (code) => {
      const current = liveRef.current;
      liveByThread.delete(input.sessionId);
      if (!current?.muteUpdates)
        current?.onEvent({ type: "session.ended", code });
      const error = new Error(
        "Antigravity CLI exited before completing the turn",
      );
      if (current) current.exitError = error;
      current?.initFailed?.(error);
      if (!current?.cancelled) current?.turnFailed?.(error);
      if (current) {
        current.initDone = null;
        current.initFailed = null;
        current.turnDone = null;
        current.turnFailed = null;
      }
    },
    (line) => console.debug("[monocode] antigravity stderr", line),
  );

  await spawnChild(
    input.sessionId,
    path,
    buildAntigravitySpawnArgs({
      model: input.model,
      modelSettings: input.modelSettings,
      runtimeMode: input.runtimeMode,
      resume,
      cwd: input.cwd,
      addDirs: allDirs,
      prompt: input.text,
    }),
    input.cwd,
  );
  liveByThread.set(input.sessionId, live);
  try {
    await waitForInit(live);
    if (!live.conversationId)
      throw new Error("Antigravity CLI started without a conversation id");
    resumeByThread.set(input.sessionId, {
      conversationId: live.conversationId,
      cwd: input.cwd,
    });
    live.onEvent({
      type: "session.providerBound",
      providerSessionId: live.conversationId,
    });
    live.onEvent({ type: "session.started" });
    return live;
  } catch (error) {
    await stopAntigravitySession(input.sessionId);
    throw error;
  }
}

async function runTurn(live: Live, input: SendTurnInput): Promise<void> {
  const message = buildAntigravityUserMessage({
    text: input.text,
    cwd: input.cwd,
    attachments: input.attachments,
  });
  if (!message) return;
  live.emittedAssistant = "";
  live.emittedThinking = "";
  const done = new Promise<void>((resolve, reject) => {
    live.turnDone = resolve;
    live.turnFailed = reject;
  });
  live.activeTurn = true;
  try {
    await writeChild(input.sessionId, JSON.stringify(message));
    await done;
  } catch (error) {
    if (live.cancelled) return;
    const message = error instanceof Error ? error.message : String(error);
    live.onEvent({ type: "session.error", message });
    throw error;
  } finally {
    live.activeTurn = false;
    live.turnDone = null;
    live.turnFailed = null;
  }
}

function handleLine(sessionId: string, live: Live, line: string): void {
  const parsed = parseAntigravityLine(line);
  if (!parsed) return;
  const mapped = mapAntigravityLine(
    parsed,
    live.emittedAssistant,
    live.contextWindow,
    live.emittedThinking,
  );
  if (
    mapped.providerSessionId &&
    mapped.providerSessionId !== live.conversationId
  ) {
    live.conversationId = mapped.providerSessionId;
    resumeByThread.set(sessionId, {
      conversationId: mapped.providerSessionId,
      cwd: live.cwd,
    });
  }
  if (mapped.initialized !== undefined) {
    live.initialized = true;
    if (live.contextWindow) {
      live.onEvent({ type: "context", window: live.contextWindow });
    }
    live.initDone?.();
    live.initDone = null;
    live.initFailed = null;
  }
  if (!live.muteUpdates) {
    for (const event of mapped.events) {
      if (event.type === "message.delta") live.emittedAssistant += event.text;
      if (event.type === "reasoning.delta") live.emittedThinking += event.text;
      if (event.type === "question.asked") {
        let threadQuestions = pendingQuestionsByThread.get(sessionId);
        if (!threadQuestions) {
          threadQuestions = new Map();
          pendingQuestionsByThread.set(sessionId, threadQuestions);
        }
        threadQuestions.set(event.requestId, event.questions);
      }
      live.onEvent(event);
    }
  }
  if (!mapped.turnCompleted) return;
  if (mapped.turnCompleted.ok) {
    live.turnDone?.();
  } else {
    live.turnFailed?.(
      new Error(mapped.turnCompleted.error ?? "Antigravity CLI turn failed"),
    );
  }
}

function waitForInit(live: Live): Promise<void> {
  if (live.initialized) return Promise.resolve();
  if (live.exitError) return Promise.reject(live.exitError);
  return new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => {
      live.initDone = null;
      live.initFailed = null;
      reject(new Error("Antigravity CLI did not initialize in time"));
    }, INIT_TIMEOUT_MS);
    live.initDone = () => {
      clearTimeout(timer);
      resolve();
    };
    live.initFailed = (error) => {
      clearTimeout(timer);
      reject(error);
    };
  });
}

function settingsKeyFor(input: SendTurnInput): string {
  const effective = effectiveAntigravitySettings({
    model: input.model,
    modelSettings: input.modelSettings,
    runtimeMode: input.runtimeMode,
    prompt: input.text,
  });
  return JSON.stringify({
    cwd: input.cwd,
    model: effective.model,
    effort: effective.effort ?? "",
    agent: effective.agent ?? "",
    mode: effective.mode ?? input.runtimeMode,
  });
}

/** Test-only cleanup. */
export async function resetAntigravityLiveForTests(): Promise<void> {
  const ids = [...new Set([...liveByThread.keys(), ...resumeByThread.keys()])];
  liveByThread.clear();
  resumeByThread.clear();
  cancelledThreads.clear();
  stagedAttachmentPaths.clear();
  await Promise.all(ids.map((id) => killChild(id).catch(() => undefined)));
  resolveAntigravityBinaryImpl = resolveAntigravityBinary;
}

async function materializeAttachments(
  sessionId: string,
  attachments: Attachment[] | undefined,
): Promise<Attachment[] | undefined> {
  if (!attachments?.some((attachment) => !attachment.path && attachment.data)) {
    return attachments;
  }
  return Promise.all(
    attachments.map(async (attachment) => {
      if (attachment.path || !attachment.data) return attachment;
      const key = `${sessionId}:${attachment.id}`;
      let path = stagedAttachmentPaths.get(key);
      if (!path) {
        path = await invoke<string>("write_attachment", {
          name: attachment.name,
          data: attachment.data,
        });
        stagedAttachmentPaths.set(key, path);
      }
      return { ...attachment, path };
    }),
  );
}

import {
  accountCredentials,
  captureExternalAccount,
  failoverAccounts,
  isQuotaWallError,
  isSharedScopeReached,
  loadCodexAccounts,
  mergeRateLimits,
  pickFailoverAccount,
  quotaResetAt,
  resetCodexAccounts,
  respondCodexRefresh,
  updateAccountState,
  type CodexAccount,
  type CodexCredentials,
  type RateLimitSnapshot,
} from "./codexAccounts";
import { nativeModelId } from "../models";
import type { RuntimeMode } from "../session";
import { questionPromptTitle, type UserQuestionReply } from "../userQuestion";
import {
  killChild,
  resolveCodexBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "./child";
import {
  asRecord,
  buildThreadStartParams,
  buildTurnStartParams,
  buildTurnSteerParams,
  isRecoverableThreadResumeError,
  mapApprovalRequest,
  codexSubagentStates,
  codexSubagentThreadIds,
  mapCodexNotification,
  mapCodexSubagentSteps,
  stringField,
  toCodexApprovalDecision,
  type CodexApprovalKind,
} from "./codexProtocol";
import { JsonRpcClient, type JsonRpcId } from "./jsonRpc";
import { codexQuestions, codexQuestionResponse } from "./codexQuestions";
import {
  codexMcpConfirmation,
  isCodexComputerUseAccessConfirmation,
} from "./codexElicitation";
import { joinStreamText, snapshotRemainder } from "./streamText";
import type {
  ApprovalDecision,
  CompactContextInput,
  HarnessEvent,
  HarnessSessionInput,
  SendTurnInput,
  SteerTurnInput,
} from "./types";

type ApprovalOutcome = ApprovalDecision | "cancelled";

type PendingApproval = {
  rpcId: JsonRpcId;
  threadId: string;
  kind: CodexApprovalKind;
  resolve: (decision: ApprovalOutcome) => void;
};

type PendingQuestion = {
  rpcId: JsonRpcId;
  threadId: string;
  event: Extract<HarnessEvent, { type: "question.asked" }>;
  isBlocking: boolean;
  timer?: ReturnType<typeof setTimeout>;
  resolve: (reply: UserQuestionReply | "cancelled") => void;
};

// Match Codex's non-blocking question policy: a minute of grace, then a
// minute of countdown. Interaction keeps the question open for the user.
const QUESTION_AUTO_RESOLVE_MS = 120_000;

// Items that provably cannot touch the workspace. Anything else — including
// types Codex adds later — is treated as effectful so failover never silently
// replays a turn whose side effects may already have happened.
const EFFECT_FREE_ITEM_TYPES = new Set([
  "agentMessage",
  "reasoning",
  "plan",
  "webSearch",
  "userMessage",
  "contextCompaction",
  "enteredReviewMode",
  "exitedReviewMode",
]);

type Live = {
  rpc: JsonRpcClient;
  threadId: string;
  cwd: string;
  runtimeMode: RuntimeMode;
  planning: boolean;
  onEvent: (event: HarnessEvent) => void;
  approvals: Map<number, PendingApproval>;
  questions: Map<number, PendingQuestion>;
  visibleQuestionId: number | null;
  nextApprovalUiId: number;
  cancelled: boolean;
  muteUpdates: boolean;
  activeTurnId: string | null;
  turns: Promise<void>;
  /** Resolves when the current turn completes (or is cancelled). */
  turnDone: (() => void) | null;
  turnFailed: ((error: Error) => void) | null;
  /** turn/completed arrived before runTurn registered turnDone. */
  turnEndPending: boolean;
  emittedAssistant: string;
  emittedReasoning: string;
  /** Child thread id -> the agent tool row that spawned it. */
  subagentThreads: Map<string, string>;
  /** Child notifications that arrived before their row was known. */
  pendingSubagent: Map<string, Array<{ method: string; params: unknown }>>;
  rateLimits?: RateLimitSnapshot;
  quotaError?: { message: string; codexErrorInfo?: unknown };
  poolEnabled: boolean;
  /** An effectful item (tool/command/file) already started in this turn —
   * replaying the input could duplicate its side effects. */
  turnHadEffects: boolean;
  /** True while a hot account login is in flight — refresh responses during
   * the gap could cross-wire credentials between accounts. */
  accountSwitching: boolean;
  /** The turn most recently started, kept after its terminal so late
   * item notifications still attribute effects to it. */
  lastTurnId: string | null;
  /** Agent rows still running, by call id, with the name to settle them under. */
  openAgentRows: Map<string, string>;
};

type Resume = {
  threadId: string;
  cwd: string;
};

const liveByThread = new Map<string, Live>();
const resumeByThread = new Map<string, Resume>();
const sessionAccount = new Map<string, string>();
const sessionTurns = new Map<string, Promise<void>>();
const failingOver = new Set<string>();
const cancelledThreads = new Set<string>();
// Cancellation epoch per session: survives Live replacement during failover,
// unlike cancelledThreads (cleared by stopCodexSession) or live.cancelled
// (bound to one process generation).
const cancelEpoch = new Map<string, number>();
const bumpCancel = (id: string) =>
  cancelEpoch.set(id, (cancelEpoch.get(id) ?? 0) + 1);
const cancelStamp = (id: string) => cancelEpoch.get(id) ?? 0;

let resolveCodexBinaryImpl: () => Promise<{ path: string }> =
  resolveCodexBinary;

/** Test seam. */
export function setCodexBinaryResolver(
  fn: () => Promise<{ path: string }>,
): void {
  resolveCodexBinaryImpl = fn;
}

// Keep the queue outside Live: a failover replaces Live and its RPC connection.
async function enqueueCodexOperation(
  sessionId: string,
  operation: () => Promise<void>,
): Promise<void> {
  const previous = sessionTurns.get(sessionId) ?? Promise.resolve();
  const task = previous.catch(() => undefined).then(operation);
  sessionTurns.set(sessionId, task);
  try {
    await task;
  } finally {
    if (sessionTurns.get(sessionId) === task) sessionTurns.delete(sessionId);
  }
}

export async function sendCodexTurn(input: SendTurnInput): Promise<void> {
  await enqueueCodexOperation(input.sessionId, () =>
    sendQueuedCodexTurn(input),
  );
}

async function sendQueuedCodexTurn(input: SendTurnInput): Promise<void> {
  await runTurnWithFailover(input);
}

export async function compactCodexContext(
  input: CompactContextInput,
): Promise<void> {
  await enqueueCodexOperation(input.sessionId, () =>
    compactQueuedCodexContext(input),
  );
}

async function compactQueuedCodexContext(
  input: CompactContextInput,
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
        await runCompaction(live);
      } catch (error) {
        if (live.cancelled) return;
        throw error;
      }
    });
  await live.turns;
}

export async function steerCodexTurn(input: SteerTurnInput): Promise<void> {
  const live = liveByThread.get(input.sessionId);
  if (!live) throw new Error("No active Codex session");
  const turnId = live.activeTurnId;
  if (!turnId) throw new Error("No active turn to steer");

  const params = buildTurnSteerParams({
    threadId: live.threadId,
    expectedTurnId: turnId,
    prompt: input.text.trim() || undefined,
    attachments: input.attachments,
  });
  if (
    !params.input ||
    (Array.isArray(params.input) && params.input.length === 0)
  ) {
    return;
  }

  await live.rpc.request("turn/steer", params);
}

export function respondCodexApproval(
  sessionId: string,
  requestId: number,
  decision: ApprovalDecision,
): void {
  const live = liveByThread.get(sessionId);
  const pending = live?.approvals.get(requestId);
  if (!pending) return;
  pending.resolve(decision);
}

export function respondCodexQuestion(
  sessionId: string,
  requestId: number,
  reply: UserQuestionReply,
): void {
  liveByThread.get(sessionId)?.questions.get(requestId)?.resolve(reply);
}

export function keepCodexQuestionOpen(
  sessionId: string,
  requestId: number,
): void {
  const live = liveByThread.get(sessionId);
  const pending = live?.questions.get(requestId);
  if (!live || !pending || pending.timer === undefined) return;
  clearTimeout(pending.timer);
  pending.timer = undefined;
  live.onEvent({ type: "question.updated", requestId });
}

function clearServerRequests(live: Live): void {
  for (const pending of live.approvals.values()) pending.resolve("cancelled");
  for (const pending of live.questions.values()) {
    clearTimeout(pending.timer);
    pending.resolve("cancelled");
  }
  live.approvals.clear();
  live.questions.clear();
  live.visibleQuestionId = null;
}

function showNextQuestion(live: Live): void {
  if (
    live.visibleQuestionId !== null &&
    live.questions.has(live.visibleQuestionId)
  )
    return;
  const next = live.questions.entries().next().value;
  live.visibleQuestionId = next?.[0] ?? null;
  if (next) {
    const pending = next[1];
    if (!pending.isBlocking) {
      pending.event.autoResolveAt = Date.now() + QUESTION_AUTO_RESOLVE_MS;
      pending.timer = setTimeout(
        () => pending.resolve({ kind: "skipped" }),
        QUESTION_AUTO_RESOLVE_MS,
      );
    }
    live.onEvent(pending.event);
  }
}

export async function cancelCodexTurn(sessionId: string): Promise<void> {
  bumpCancel(sessionId);
  const live = liveByThread.get(sessionId);
  if (!live) {
    cancelledThreads.add(sessionId);
    return;
  }
  live.cancelled = true;
  live.muteUpdates = true;
  clearServerRequests(live);
  const turnId = live.activeTurnId;
  if (turnId) {
    await live.rpc
      .request(
        "turn/interrupt",
        {
          threadId: live.threadId,
          turnId,
        },
        5_000,
      )
      .catch(() => undefined);
  }
  finishActiveTurn(live, [
    { type: "message.completed" },
    { type: "reasoning.completed" },
  ]);
}

export async function stopCodexSession(sessionId: string): Promise<void> {
  cancelledThreads.delete(sessionId);
  const live = liveByThread.get(sessionId);
  liveByThread.delete(sessionId);
  if (live) {
    live.muteUpdates = true;
    clearServerRequests(live);
    live.turnDone?.();
    live.turnDone = null;
    live.turnFailed = null;
    live.rpc.close();
  }
  unwatchChild(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

export async function forgetCodexSession(sessionId: string): Promise<void> {
  sessionAccount.delete(sessionId);
  resumeByThread.delete(sessionId);
  cancelEpoch.delete(sessionId);
  await stopCodexSession(sessionId);
}

export function bindCodexSession(
  threadId: string,
  providerSessionId: string,
  cwd: string,
): void {
  const providerThreadId = providerSessionId.trim();
  if (!threadId || !providerThreadId || !cwd.trim()) return;
  resumeByThread.set(threadId, { threadId: providerThreadId, cwd });
}

async function ensureLive(input: HarnessSessionInput): Promise<Live> {
  const existing = liveByThread.get(input.sessionId);
  if (existing && existing.cwd === input.cwd) {
    existing.onEvent = input.onEvent;
    return existing;
  }
  if (existing) {
    resumeByThread.delete(input.sessionId);
    await stopCodexSession(input.sessionId);
  }

  const resume = resumeByThread.get(input.sessionId);
  const accounts = await loadCodexAccounts().catch(() => []);
  const canResume = resume != null && resume.cwd === input.cwd;
  if (resume && resume.cwd !== input.cwd) {
    resumeByThread.delete(input.sessionId);
  }

  const { path } = await resolveCodexBinaryImpl();
  const liveRef: { current: Live | null } = { current: null };

  const rpc = new JsonRpcClient(
    input.sessionId,
    {
      onNotification: (method, params) => {
        const live = liveRef.current;
        if (!live || live.muteUpdates) return;
        handleNotification(live, method, params);
      },
      onRequest: (id, method, params) => {
        if (method === "account/chatgptAuthTokens/refresh") {
          // A hot switch can re-bind the process between request arrival and
          // response: only answer while the pinned account is unchanged.
          const pinAtRequest = sessionAccount.get(input.sessionId);
          void respondCodexRefresh(
            rpc,
            id,
            params,
            pinAtRequest,
            () =>
              sessionAccount.get(input.sessionId) === pinAtRequest &&
              !liveRef.current?.accountSwitching,
          ).catch(() => undefined);
          return;
        }
        const live = liveRef.current;
        const turn = live?.turnDone;
        // The external clock can be requested before thread/start or resume
        // returns, so it must not depend on the live session being bound.
        const response =
          method === "currentTime/read"
            ? rpc.respond(id, { currentTimeAt: Math.floor(Date.now() / 1000) })
            : live
              ? handleServerRequest(live, id, method, params)
              : undefined;
        void response?.catch((error: unknown) => {
          if (live?.muteUpdates || (live && live.turnDone !== turn)) return;
          const failure =
            error instanceof Error ? error : new Error(String(error));
          if (live?.turnFailed) {
            live.turnFailed(failure);
          } else {
            (live?.onEvent ?? input.onEvent)({
              type: "session.error",
              message: failure.message,
            });
          }
        });
      },
    },
    { includeJsonrpc: false, label: "codex" },
  );

  watchChild(
    input.sessionId,
    (line) => rpc.pushLine(line),
    (code) => {
      rpc.close(new Error("Codex app-server exited"));
      if (liveByThread.get(input.sessionId) === liveRef.current)
        liveByThread.delete(input.sessionId);
      const live = liveRef.current;
      if (!live?.muteUpdates) {
        (live?.onEvent ?? input.onEvent)({ type: "session.ended", code });
      }
      live?.turnFailed?.(new Error("Codex app-server exited"));
      if (live) {
        clearServerRequests(live);
        live.turnDone = null;
        live.turnFailed = null;
      }
    },
  );

  rpc.expectedPid = await spawnChild(
    input.sessionId,
    path,
    ["app-server"],
    input.cwd,
  );

  try {
    await rpc.request("initialize", {
      clientInfo: {
        name: "monocode",
        title: "MonoCode",
        version: "0.1.0",
      },
      capabilities: {
        // Required by collaborationMode (including Plan); currentTime/read is
        // handled above even while the thread is starting or resuming.
        experimentalApi: true,
      },
    });
    await rpc.notify("initialized", undefined);
    const pinned = sessionAccount.get(input.sessionId);
    if (pinned) {
      try {
        const credentials = await accountCredentials(pinned).catch(
          (error: unknown) => {
            const wrapped: CodexError =
              error instanceof Error ? error : new Error(String(error));
            // Unavailable credentials are the account's fault unless flagged
            // transient (e.g. a network failure during token refresh).
            wrapped.accountRejected = wrapped.permanent !== false;
            wrapped.accountDisabled = wrapped.permanent !== false;
            throw wrapped;
          },
        );
        await codexAccountLogin(rpc, credentials);
        await codexVerifyIdentity(rpc, credentials);
        await updateAccountState(pinned, { lastUsedAt: Date.now() });
      } catch (error) {
        // A rejected account must not poison the session pin: unpin it, and
        // only disable it when the rejection is definitively its own.
        sessionAccount.delete(input.sessionId);
        if ((error as CodexError).accountDisabled) {
          await updateAccountState(pinned, {
            disabledCause:
              error instanceof Error ? error.message : String(error),
          }).catch(() => undefined);
        }
        throw error;
      }
    }

    const model = nativeModelId(input.model);
    const serviceTier = input.modelSettings?.serviceTier;
    const effort = input.modelSettings?.reasoningEffort;

    let threadId: string | undefined;
    let didResume = false;

    if (canResume && resume) {
      try {
        const opened = await rpc.request<{ thread?: { id?: string } }>(
          "thread/resume",
          {
            threadId: resume.threadId,
            ...buildThreadStartParams({
              cwd: input.cwd,
              runtimeMode: input.runtimeMode,
              controlsAgents: input.controlsAgents,
              model,
              serviceTier,
            }),
          },
        );
        threadId = opened.thread?.id ?? resume.threadId;
        didResume = true;
      } catch (error) {
        if (failingOver.has(input.sessionId)) {
          // Fail closed: silently starting a fresh thread would drop the
          // conversation and make replaying the turn even less safe.
          throw new CodexResumeError(
            "Codex account switched but the thread could not be restored — start a new conversation",
          );
        }
        if (!isRecoverableThreadResumeError(error)) throw error;
        threadId = undefined;
      }
    }

    if (!threadId) {
      const opened = await rpc.request<{ thread?: { id?: string } }>(
        "thread/start",
        buildThreadStartParams({
          cwd: input.cwd,
          runtimeMode: input.runtimeMode,
          controlsAgents: input.controlsAgents,
          model,
          serviceTier,
        }),
      );
      threadId = opened.thread?.id?.trim();
    }

    if (!threadId) throw new Error("Codex did not return a thread id");

    // Suppress unused warning for effort until first turn applies it.
    void effort;

    const live: Live = {
      rpc,
      threadId,
      cwd: input.cwd,
      runtimeMode: input.runtimeMode,
      planning: input.intent === "plan",
      onEvent: input.onEvent,
      approvals: new Map(),
      questions: new Map(),
      visibleQuestionId: null,
      nextApprovalUiId: 1,
      cancelled: false,
      muteUpdates: didResume,
      activeTurnId: null,
      turns: Promise.resolve(),
      turnDone: null,
      turnFailed: null,
      turnEndPending: false,
      turnHadEffects: false,
      accountSwitching: false,
      lastTurnId: null,
      emittedAssistant: "",
      emittedReasoning: "",
      subagentThreads: new Map(),
      pendingSubagent: new Map(),
      openAgentRows: new Map(),
      poolEnabled: accounts.length > 0,
    };
    liveRef.current = live;
    liveByThread.set(input.sessionId, live);
    resumeByThread.set(input.sessionId, {
      threadId,
      cwd: input.cwd,
    });
    live.onEvent({
      type: "session.providerBound",
      providerSessionId: threadId,
    });
    live.onEvent({ type: "session.started" });
    return live;
  } catch (error) {
    rpc.close(error instanceof Error ? error : new Error(String(error)));
    await stopCodexSession(input.sessionId);
    throw error;
  }
}

type CodexError = Error & {
  /** The account itself was rejected — failover should try a sibling. */
  accountRejected?: boolean;
  /** The rejection is provably the account's fault — persist disabledCause.
   * Ambiguous RPC failures only rotate for this send; a protocol drift must
   * not permanently retire every healthy account in the pool. */
  accountDisabled?: boolean;
  /** The failure permanently invalidates the credential (dead refresh). */
  permanent?: boolean;
};

class CodexResumeError extends Error {}

// RPC-level rejections blame the account; transport failures blame the
// process and must not retire a healthy account.
function isCodexTransportError(error: unknown): boolean {
  return /timed out|not running|exited|closed|replaced|cancelled/i.test(
    error instanceof Error ? error.message : String(error),
  );
}

async function codexAccountLogin(
  rpc: Pick<JsonRpcClient, "request">,
  credentials: CodexCredentials,
): Promise<void> {
  await rpc
    .request(
      "account/login/start",
      {
        type: "chatgptAuthTokens",
        accessToken: credentials.accessToken,
        chatgptAccountId: credentials.accountId,
        ...(credentials.planType
          ? { chatgptPlanType: credentials.planType }
          : {}),
      },
      10_000,
    )
    .catch((error: unknown) => {
      const wrapped: CodexError = new Error("Codex account login failed");
      wrapped.accountRejected = !isCodexTransportError(error);
      throw wrapped;
    });
}

async function codexVerifyIdentity(
  rpc: Pick<JsonRpcClient, "request">,
  credentials: CodexCredentials,
): Promise<void> {
  const identity = await rpc
    .request<{ account?: { email?: string } }>("account/read", {}, 10_000)
    .catch((error: unknown) => {
      const wrapped: CodexError = new Error(
        "Codex account verification failed",
      );
      wrapped.accountRejected = !isCodexTransportError(error);
      throw wrapped;
    });
  if (
    !identity.account?.email ||
    identity.account.email.toLowerCase() !== credentials.email.toLowerCase()
  ) {
    const wrapped: CodexError = new Error("Codex account verification failed");
    wrapped.accountRejected = true;
    // A verified identity mismatch is provably the credential's fault.
    wrapped.accountDisabled = true;
    throw wrapped;
  }
}

// "rejected" means the credentials are permanently unusable (dead refresh
// token); anything else may be a process problem, so the respawn path makes
// the definitive call instead of blaming the account.
async function tryHotAccountSwitch(
  live: Live,
  sessionId: string,
  next: CodexAccount,
): Promise<"ok" | "rejected" | "unavailable"> {
  let credentials: CodexCredentials;
  try {
    credentials = await accountCredentials(next.id);
  } catch (error) {
    return (error as CodexError).permanent === false
      ? "unavailable"
      : "rejected";
  }
  try {
    live.accountSwitching = true;
    try {
      await codexAccountLogin(live.rpc, credentials);
      await codexVerifyIdentity(live.rpc, credentials);
    } finally {
      live.accountSwitching = false;
    }
  } catch {
    return "unavailable";
  }
  sessionAccount.set(sessionId, next.id);
  await updateAccountState(next.id, { lastUsedAt: Date.now() });
  return "ok";
}

async function runTurnWithFailover(input: SendTurnInput): Promise<void> {
  // Cancellation must survive Live replacement: a cancel issued while the old
  // process is down (hot-switch rejection, respawn) still aborts the send.
  const epoch = cancelStamp(input.sessionId);
  // cancelledThreads is consumed on observation: a marker left by a cancel
  // with no live process aborts the next send once, not every future one.
  const cancelled = () =>
    cancelStamp(input.sessionId) !== epoch ||
    cancelledThreads.delete(input.sessionId);
  let live: Live | undefined;
  const budget = { used: 0, max: 0 };
  try {
    for (;;) {
      if (!live) {
        try {
          live = await ensureLive(input);
        } catch (error) {
          cancelledThreads.delete(input.sessionId);
          // A definitively rejected pinned identity fails over like a wall;
          // so does a transient credential failure (a sibling may still be
          // usable). Only process/transport failures stay ordinary errors.
          if (
            !(error as CodexError).accountRejected &&
            (error as CodexError).permanent !== false
          )
            throw error;
          const accounts = await loadCodexAccounts().catch(() => []);
          if (!accounts.length) throw error;
          budget.max = accounts.length;
          live = await failoverAcquire(input, undefined, budget, cancelled);
          if (!live) {
            if (cancelled()) return;
            throw codexExhaustedError();
          }
        }
        live.onEvent = input.onEvent;
        if (cancelled() || cancelledThreads.delete(input.sessionId)) return;
        live.cancelled = false;
      }
      live.poolEnabled = (await loadCodexAccounts().catch(() => [])).length > 0;
      // A cancel can land while accounts load — re-check before submitting.
      if (cancelled() || live.cancelled) return;
      live.runtimeMode = input.runtimeMode;
      live.planning = input.intent === "plan";
      live.muteUpdates = false;
      try {
        await runTurn(live, input);
        return;
      } catch (error) {
        if (cancelled() || live.cancelled) return;
        const message = error instanceof Error ? error.message : String(error);
        const info = live.quotaError?.codexErrorInfo;
        if (!isQuotaWallError(info, message)) throw error;
        const accounts = await loadCodexAccounts().catch(() => []);
        if (!accounts.length) throw error;
        // Pushed snapshots are sparse and can lag a hot switch; the wall
        // decision (shared vs account-scoped) needs an authoritative read.
        const limits = await live.rpc
          .request("account/rateLimits/read", {}, 5_000)
          .then((response) => asRecord(asRecord(response)?.rateLimits))
          .catch(() => undefined);
        if (limits)
          live.rateLimits = mergeRateLimits(
            live.rateLimits,
            limits as RateLimitSnapshot,
          );
        // A shared (workspace_*) limit blocks only this workspace — siblings
        // in other workspaces still work, so rotation proceeds either way.
        // Candidates are validated post-switch instead of assumed usable.
        budget.max = Math.max(budget.max, accounts.length);
        let current = sessionAccount.get(input.sessionId);
        if (!current) {
          // External credentials stay in memory; never import them into the pool.
          try {
            await captureExternalAccount(live.rpc);
            current = "external";
            sessionAccount.set(input.sessionId, current);
          } catch {
            /* API-key/unknown external identity is not a return candidate. */
          }
        }
        if (current)
          await updateAccountState(current, {
            blockedUntilMs: quotaResetAt(live.rateLimits),
          });
        const acquired = await failoverAcquire(input, live, budget, cancelled);
        if (!acquired) {
          if (cancelled() || cancelledThreads.delete(input.sessionId)) return;
          throw codexExhaustedError();
        }
        // Read the flag only after failover settles: late item notifications
        // can still land on the old process while the replacement spins up.
        const hadEffects = live.turnHadEffects;
        live = acquired;
        if (cancelled() || cancelledThreads.delete(input.sessionId)) return;
        if (hadEffects) {
          // Never silently replay a turn that may already have run tools:
          // thread/resume gives no exactly-once guarantee, so the user must
          // review the workspace and resend explicitly.
          const pinnedId = sessionAccount.get(input.sessionId);
          const email =
            failoverAccounts().find((a) => a.id === pinnedId)?.email ??
            "another account";
          throw new Error(
            `Codex account exhausted — switched to ${email}, but the interrupted turn may already have run tools. Review the workspace, then resend.`,
          );
        }
      }
    }
  } catch (error) {
    input.onEvent({
      type: "session.error",
      message: error instanceof Error ? error.message : String(error),
    });
    throw error;
  }
}

function codexExhaustedError(): Error {
  const resets = failoverAccounts()
    .filter((a) => !a.disabledCause && (a.blockedUntilMs ?? 0) > Date.now())
    .map((a) => a.blockedUntilMs!);
  return new Error(
    `Codex accounts exhausted${resets.length ? ` — earliest reset ${new Date(Math.min(...resets)).toISOString()}` : " — sign in again"}`,
  );
}

// A candidate can be walled too — a shared workspace limit follows every
// account inside that workspace. Ask the authoritative endpoint rather than
// spending the user's turn to find out.
async function accountUsableNow(live: Live): Promise<boolean> {
  const response = await live.rpc
    .request("account/rateLimits/read", {}, 5_000)
    .then((r) => asRecord(r))
    .catch(() => undefined);
  if (!response) return true; // Cannot tell — let the turn decide.
  const limits = asRecord(response.rateLimits);
  if (limits)
    live.rateLimits = mergeRateLimits(
      live.rateLimits,
      limits as RateLimitSnapshot,
    );
  return (
    response.ordinaryUsageAllowed !== false &&
    !isSharedScopeReached(limits?.rateLimitReachedType)
  );
}

// Pick the next sibling and move the session onto it: hot login first (no
// process restart), with the verified respawn+resume path as the definitive
// fallback. Returns undefined when every candidate is spent or cancelled.
async function failoverAcquire(
  input: SendTurnInput,
  live: Live | undefined,
  budget: { used: number; max: number },
  cancelled: () => boolean,
): Promise<Live | undefined> {
  // Accounts rotated past without a persisted disable (ambiguous RPC
  // rejections) must not be re-picked within this send.
  const tried = new Set<string>();
  while (budget.used < budget.max) {
    if (cancelled()) return undefined;
    const next = pickFailoverAccount(
      failoverAccounts(),
      sessionAccount.get(input.sessionId),
      Date.now(),
      tried,
    );
    if (!next) return undefined;
    budget.used++;
    tried.add(next.id);
    input.onEvent({
      type: "status",
      text: `Codex account exhausted — switching to ${next.email}`,
    });
    if (live) {
      const hot = await tryHotAccountSwitch(live, input.sessionId, next);
      if (hot === "ok") {
        // The old snapshot described the walled account.
        live.rateLimits = undefined;
        if (await accountUsableNow(live)) return live;
        // Walled too (e.g. same workspace's shared credits) — block it and
        // move on; the process stays usable for the next hot switch.
        await updateAccountState(next.id, {
          blockedUntilMs: quotaResetAt(live.rateLimits),
        }).catch(() => undefined);
        continue;
      }
      if (hot === "rejected") {
        await updateAccountState(next.id, {
          disabledCause: "Codex account credentials unavailable",
        }).catch(() => undefined);
        continue;
      }
    }
    await stopCodexSession(input.sessionId);
    sessionAccount.set(input.sessionId, next.id);
    failingOver.add(input.sessionId);
    try {
      const acquired = await ensureLive(input);
      if (await accountUsableNow(acquired)) return acquired;
      await updateAccountState(next.id, {
        blockedUntilMs: quotaResetAt(acquired.rateLimits),
      }).catch(() => undefined);
      // The respawned process is healthy — keep it so the next candidate can
      // hot-switch without paying for another spawn.
      live = acquired;
      continue;
    } catch (error) {
      if (error instanceof CodexResumeError) throw error;
      // ensureLive unpins definitively rejected identities; transient
      // credential failures skip this candidate without retiring it, while
      // spawn/transport failures stay ordinary errors.
      if (!(error as CodexError).accountRejected) {
        if ((error as CodexError).permanent === false) continue;
        throw error;
      }
    } finally {
      failingOver.delete(input.sessionId);
    }
  }
  return undefined;
}

async function runTurn(live: Live, input: SendTurnInput): Promise<void> {
  const model = nativeModelId(input.model);
  const effort = input.modelSettings?.reasoningEffort;
  const serviceTier = input.modelSettings?.serviceTier;

  const params = buildTurnStartParams({
    threadId: live.threadId,
    runtimeMode: input.runtimeMode,
    controlsAgents: input.controlsAgents,
    prompt: input.text.trim() || undefined,
    attachments: input.attachments,
    model,
    effort,
    serviceTier,
    intent: input.intent,
  });

  if (Array.isArray(params.input) && params.input.length === 0) {
    return;
  }

  live.emittedAssistant = "";
  live.emittedReasoning = "";
  live.quotaError = undefined;
  live.turnHadEffects = false;

  const turnPromise = new Promise<void>((resolve, reject) => {
    live.turnDone = resolve;
    live.turnFailed = reject;
  });
  // A process/request failure can reject this before turn/start has answered.
  void turnPromise.catch(() => undefined);
  settlePendingTurn(live);

  try {
    const response = await live.rpc.request<{ turn?: { id?: string } }>(
      "turn/start",
      params,
    );
    const turnId = response.turn?.id;
    if (turnId) live.lastTurnId = turnId;
    // turn/completed may already have raced ahead of this continuation; only
    // track the id while a waiter exists so a stale id can't arm a cancel.
    if (turnId && live.turnDone) {
      live.activeTurnId = live.activeTurnId ?? turnId;
    }
    settlePendingTurn(live);
    await turnPromise;
    const quotaError = live.quotaError as Live["quotaError"];
    if (quotaError) throw new Error(quotaError.message);
  } catch (error) {
    if (live.cancelled) return;
    // The outer loop decides whether this is terminal or an account failover.
    throw error;
  } finally {
    live.turnDone = null;
    live.turnFailed = null;
  }
}

async function runCompaction(live: Live): Promise<void> {
  live.emittedAssistant = "";
  live.emittedReasoning = "";
  const turnPromise = new Promise<void>((resolve, reject) => {
    live.turnDone = resolve;
    live.turnFailed = reject;
  });
  settlePendingTurn(live);

  try {
    await live.rpc.request("thread/compact/start", {
      threadId: live.threadId,
    });
    settlePendingTurn(live);
    await turnPromise;
  } finally {
    live.turnDone = null;
    live.turnFailed = null;
  }
}

function handleNotification(live: Live, method: string, params: unknown): void {
  const rec = asRecord(params);
  if (method === "serverRequest/resolved") {
    for (const pending of live.approvals.values()) {
      if (
        pending.rpcId === rec?.requestId &&
        pending.threadId === rec?.threadId
      )
        pending.resolve("cancelled");
    }
    for (const pending of live.questions.values()) {
      if (
        pending.rpcId === rec?.requestId &&
        pending.threadId === rec?.threadId
      )
        pending.resolve("cancelled");
    }
    return;
  }
  // A turn that already started effectful items (tools, commands, edits —
  // including inside child threads, which run on this same connection) must
  // never be replayed silently, so this check runs before the threadId split.
  // Items carry turnId: effects arriving after turnDone was cleared still
  // attribute to the turn they belong to.
  if (
    (method === "item/started" || method === "item/completed") &&
    !EFFECT_FREE_ITEM_TYPES.has(
      stringField(asRecord(rec?.item), "type") ?? "",
    ) &&
    (live.turnDone != null ||
      (stringField(rec, "turnId") != null &&
        stringField(rec, "turnId") === live.lastTurnId))
  )
    live.turnHadEffects = true;
  // Child threads share this connection. Their lifecycle must not touch the
  // parent's turn or clear its approvals, but what they do is the inside of a
  // subagent — mirror it onto the row that spawned them.
  const threadId =
    stringField(rec, "threadId") ??
    (method === "thread/started"
      ? stringField(asRecord(rec?.thread), "id")
      : undefined);
  if (threadId && threadId !== live.threadId) {
    handleSubagentNotification(live, threadId, method, params);
    return;
  }
  // A Codex turn is a sequence of items. Completing an agentMessage does not
  // mean the turn is over — more tools and messages can still arrive. Only
  // turn/completed (and turn/aborted) settle sendCodexTurn, which is what the
  // UI uses for busy / stop / "Working for".
  const mapped = mapCodexNotification(method, params);
  // A terminal notification for a superseded turn (e.g. the interrupted
  // turn's late turn/aborted) must not complete the turn now running.
  if (
    mapped.turnCompleted &&
    mapped.terminalTurnId != null &&
    live.activeTurnId != null &&
    mapped.terminalTurnId !== live.activeTurnId
  )
    return;
  if (mapped.diagnostic) {
    // Diagnostics must not include authentication request/response payloads.
    console.debug(
      `[monocode] codex ${live.threadId} ${method}`,
      mapped.diagnostic,
    );
  }
  if (mapped.rateLimits)
    live.rateLimits = mergeRateLimits(live.rateLimits, mapped.rateLimits);
  const terminal = mapped.turnCompleted;
  const quotaWall =
    terminal &&
    live.poolEnabled &&
    isQuotaWallError(terminal.codexErrorInfo, terminal.error);
  if (quotaWall) {
    live.quotaError = {
      message: terminal.error ?? "Codex usage limit exceeded",
      codexErrorInfo: terminal.codexErrorInfo,
    };
  }
  // Codex describes one spawned agent through more than one item type. The
  // first row to name a child thread owns it; a later item for the same thread
  // would otherwise stand up a second agent that never does anything.
  const duplicate = bindSubagentThreads(live, method, rec);
  const snapshot = method === "item/completed";
  for (const event of mapped.events) {
    if (
      event.type === "session.error" &&
      live.poolEnabled &&
      (quotaWall ||
        (method === "error" &&
          isQuotaWallError(
            asRecord(rec?.error)?.codexErrorInfo,
            event.message,
          )))
    )
      continue;
    if (duplicate && duplicateAgentRow(event)) continue;
    trackAgentRow(live, event);
    if (event.type === "message.delta") {
      publishCodexText(live, "assistant", event.text, snapshot);
      continue;
    }
    if (event.type === "reasoning.delta") {
      publishCodexText(live, "reasoning", event.text, snapshot);
      continue;
    }
    live.onEvent(event);
  }
  // Metadata and steps can arrive before the spawn. Create its row first.
  for (const childId of codexSubagentThreadIds(asRecord(rec?.item) ?? {})) {
    const owner = live.subagentThreads.get(childId);
    if (!owner) continue;
    const backlog = live.pendingSubagent.get(childId);
    live.pendingSubagent.delete(childId);
    for (const pending of backlog ?? [])
      emitSubagentSteps(live, owner, pending.method, pending.params);
  }
  settleSubagentRows(live, rec);
  if (mapped.activeTurnId !== undefined) {
    live.activeTurnId = mapped.activeTurnId;
    if (mapped.activeTurnId) live.lastTurnId = mapped.activeTurnId;
  }
  if (mapped.terminalTurnId) live.lastTurnId = mapped.terminalTurnId;
  if (mapped.turnCompleted) {
    finishActiveTurn(live);
  }
}

/**
 * How many notifications a not-yet-identified child thread may bank. Codex can
 * stream a subagent's first calls before the spawn item reports which thread it
 * created, and those calls are the most interesting ones — but an unrecognised
 * thread must not be able to grow this without bound.
 */
const MAX_PENDING_SUBAGENT = 64;

/**
 * Learns which agent row a child thread belongs to.
 * Returns true when every thread this item names already belongs to another
 * row, which makes the item a second description of an agent we already show.
 */
function bindSubagentThreads(
  live: Live,
  method: string,
  rec: Record<string, unknown> | null,
): boolean {
  if (method !== "item/started" && method !== "item/completed") return false;
  const item = asRecord(rec?.item);
  if (!item) return false;
  const itemType = stringField(item, "type") ?? "";
  if (itemType !== "subAgentActivity" && itemType !== "collabAgentToolCall") {
    return false;
  }
  const callId = stringField(item, "id");
  if (!callId) return false;
  const children = codexSubagentThreadIds(item).filter(
    (childId) => childId !== live.threadId,
  );
  let claimed = 0;
  for (const childId of children) {
    const owner = live.subagentThreads.get(childId);
    if (owner) {
      const model = stringField(item, "model");
      if (model && item.tool === "spawnAgent")
        live.onEvent({
          type: "tool.updated",
          callId: owner,
          kind: "agent",
          agentModel: model,
        });
      if (owner !== callId) claimed += 1;
      continue;
    }
    live.subagentThreads.set(childId, callId);
  }
  return children.length > 0 && claimed === children.length;
}

/**
 * An agent row for a child thread another row already owns. A failure still
 * gets its row — the reason a run died is the one thing worth a line of its
 * own — but a duplicate "running" or "done" is just noise.
 */
function duplicateAgentRow(event: HarnessEvent): boolean {
  if (event.type !== "tool.started" && event.type !== "tool.updated") {
    return false;
  }
  return event.kind === "agent" && event.status !== "failed";
}

/**
 * A child thread's notification. Until the spawn item says which row the thread
 * belongs to, keep it: dropping it loses the opening moves of the run.
 */
function handleSubagentNotification(
  live: Live,
  threadId: string,
  method: string,
  params: unknown,
): void {
  const callId = live.subagentThreads.get(threadId);
  if (callId) {
    emitSubagentSteps(live, callId, method, params);
    return;
  }
  if (
    method !== "item/started" &&
    method !== "item/completed" &&
    method !== "thread/started"
  )
    return;
  const backlog = live.pendingSubagent.get(threadId) ?? [];
  if (backlog.length >= MAX_PENDING_SUBAGENT) return;
  backlog.push({ method, params });
  live.pendingSubagent.set(threadId, backlog);
}

function emitSubagentSteps(
  live: Live,
  callId: string,
  method: string,
  params: unknown,
): void {
  for (const event of mapCodexSubagentSteps(callId, method, params)) {
    live.onEvent(event);
  }
}

/** Remembers an agent row while it runs, so the turn can close it out. */
function trackAgentRow(live: Live, event: HarnessEvent): void {
  if (event.type !== "tool.started" && event.type !== "tool.updated") return;
  if (event.kind !== "agent") return;
  if (event.status === "in_progress" || event.status === "pending") {
    live.openAgentRows.set(event.callId, event.title ?? "Subagent");
    return;
  }
  live.openAgentRows.delete(event.callId);
}

/**
 * Settles spawned agents from the per-agent state a collab item reports. The
 * spawn call returns immediately; this is the first word on whether the agent
 * it started actually finished.
 */
function settleSubagentRows(
  live: Live,
  rec: Record<string, unknown> | null,
): void {
  const item = asRecord(rec?.item);
  if (!item) return;
  for (const state of codexSubagentStates(item)) {
    const callId = live.subagentThreads.get(state.threadId);
    const title = callId ? live.openAgentRows.get(callId) : undefined;
    if (!callId || !title) continue;
    live.openAgentRows.delete(callId);
    live.onEvent({
      type: "tool.updated",
      callId,
      title,
      kind: "agent",
      status: state.status,
      ...(state.message ? { detail: state.message } : {}),
    });
  }
}

/**
 * A turn cannot end with an agent still working. Codex does not always report
 * a closing state for every child, and a row left running would hop forever.
 */
function closeOpenAgentRows(live: Live): void {
  for (const [callId, title] of live.openAgentRows) {
    live.onEvent({
      type: "tool.updated",
      callId,
      title,
      kind: "agent",
      status: "completed",
    });
  }
  live.openAgentRows.clear();
}

function publishCodexText(
  live: Live,
  role: "assistant" | "reasoning",
  text: string,
  snapshot: boolean,
): void {
  const already =
    role === "assistant" ? live.emittedAssistant : live.emittedReasoning;
  const emit = snapshot ? snapshotRemainder(already, text) : text;
  if (!emit) return;
  if (role === "assistant") {
    live.emittedAssistant = joinStreamText(already, emit);
    live.onEvent({ type: "message.delta", text: emit });
    return;
  }
  live.emittedReasoning = joinStreamText(already, emit);
  live.onEvent({ type: "reasoning.delta", text: emit });
}

function finishActiveTurn(live: Live, extraEvents: HarnessEvent[] = []): void {
  clearServerRequests(live);
  closeOpenAgentRows(live);
  // The pending latch may only arm for a genuinely in-flight turn; a late
  // turn/completed (e.g. for an already-cancelled turn) must not resolve the
  // next send before it runs.
  const hadActiveTurn = live.activeTurnId !== null;
  live.turnEndPending = false;
  live.activeTurnId = null;
  live.emittedAssistant = "";
  live.emittedReasoning = "";
  for (const event of extraEvents) {
    live.onEvent(event);
  }
  const done = live.turnDone;
  const failed = live.turnFailed;
  live.turnDone = null;
  live.turnFailed = null;
  if (done) {
    done();
    return;
  }
  if (!failed && hadActiveTurn) {
    live.turnEndPending = true;
  }
}

function settlePendingTurn(live: Live): void {
  if (!live.turnEndPending || !live.turnDone) return;
  finishActiveTurn(live);
}

async function handleServerRequest(
  live: Live,
  id: JsonRpcId,
  method: string,
  params: unknown,
): Promise<void> {
  const threadId = stringField(asRecord(params), "threadId") ?? live.threadId;
  if (method === "item/tool/requestUserInput") {
    if (live.cancelled || live.muteUpdates) {
      await live.rpc.respond(id, { answers: {} });
      return;
    }
    let questions;
    try {
      questions = codexQuestions(params);
    } catch (error) {
      live.onEvent({
        type: "status",
        text: error instanceof Error ? error.message : String(error),
      });
      await live.rpc.respond(id, { answers: {} });
      return;
    }
    const uiId = live.nextApprovalUiId++;
    const event: Extract<HarnessEvent, { type: "question.asked" }> = {
      type: "question.asked",
      requestId: uiId,
      title: questionPromptTitle(questions),
      questions,
      callId: stringField(asRecord(params), "itemId"),
    };
    const outcome = new Promise<UserQuestionReply | "cancelled">((resolve) => {
      live.questions.set(uiId, {
        rpcId: id,
        threadId,
        event,
        resolve,
        // Older servers omit this field and must keep their blocking behavior.
        isBlocking: asRecord(params)?.isBlocking !== false,
      });
    }).finally(() => {
      clearTimeout(live.questions.get(uiId)?.timer);
      live.questions.delete(uiId);
    });
    showNextQuestion(live);
    const reply = await outcome;
    live.onEvent({
      type: "question.resolved",
      requestId: uiId,
      decision:
        reply === "cancelled"
          ? "cancelled"
          : reply.kind === "answered"
            ? "answered"
            : "skipped",
    });
    showNextQuestion(live);
    if (reply !== "cancelled")
      await live.rpc.respond(id, codexQuestionResponse(questions, reply));
    return;
  }

  if (method === "mcpServer/elicitation/request") {
    const confirmation = codexMcpConfirmation(params);
    if (!confirmation || live.cancelled || live.muteUpdates) {
      if (!live.cancelled && !live.muteUpdates)
        live.onEvent({
          type: "status",
          text: "This MCP server requested a form or browser sign-in that MonoCode does not support yet. Complete it in the server's own interface.",
        });
      await live.rpc.respond(id, {
        action: "cancel",
        content: null,
        _meta: null,
      });
      return;
    }
    if (
      !live.planning &&
      live.runtimeMode === "full-access" &&
      isCodexComputerUseAccessConfirmation(params)
    ) {
      await live.rpc.respond(id, {
        action: "accept",
        content: confirmation.content,
        _meta: null,
      });
      return;
    }
    const uiId = live.nextApprovalUiId++;
    const pending = waitApproval(live, uiId, id, "permissions", threadId);
    // Other MCP consent must carry the user's decision, including in Full Access.
    live.onEvent({
      type: "approval.requested",
      requestId: uiId,
      kind: "other",
      title: confirmation.title,
    });
    const decision = await pending;
    live.onEvent({ type: "approval.resolved", requestId: uiId, decision });
    if (decision !== "cancelled")
      await live.rpc.respond(id, {
        action: decision === "allow" ? "accept" : "decline",
        content: decision === "allow" ? confirmation.content : null,
        _meta: null,
      });
    return;
  }

  const uiId = live.nextApprovalUiId++;
  const mapped = mapApprovalRequest(method, params, uiId);
  if (!mapped) {
    // An empty success or invented denial hides protocol incompatibility.
    live.onEvent({
      type: "status",
      text: `Unsupported Codex request: ${method}`,
    });
    await live.rpc.respondError(id, {
      code: -32601,
      message: `Unsupported method: ${method}`,
    });
    return;
  }

  if (live.planning || live.cancelled || live.muteUpdates) {
    // Plan turns run in a non-escalating read-only sandbox. If an older
    // app-server still asks for broader access, deny it silently instead of
    // leaking a Supervised approval prompt into the user's selected mode.
    if (method === "item/permissions/requestApproval") {
      await live.rpc.respond(id, { permissions: {} }).catch(() => undefined);
    } else {
      await live.rpc
        .respond(id, {
          decision: toCodexApprovalDecision("deny", mapped.kind),
        })
        .catch(() => undefined);
    }
    return;
  }

  if (method === "item/permissions/requestApproval") {
    // Auto-deny extra permission grants in supervised; allow in full-access.
    if (live.runtimeMode === "full-access") {
      const rec = asRecord(params);
      const permissions = rec?.permissions ?? {};
      await live.rpc.respond(id, {
        scope: "session",
        permissions,
      });
      return;
    }
    if (live.runtimeMode === "supervised") {
      const pending = waitApproval(live, uiId, id, mapped.kind, threadId);
      live.onEvent(mapped.event);
      const decision = await pending;
      live.onEvent({
        type: "approval.resolved",
        requestId: uiId,
        decision,
      });
      if (decision === "cancelled") return;
      if (decision === "allow") {
        const rec = asRecord(params);
        await live.rpc.respond(id, {
          scope: "turn",
          permissions: rec?.permissions ?? {},
        });
      } else {
        await live.rpc.respond(id, { permissions: {} });
      }
      return;
    }
    // auto / auto-accept: grant requested permissions for the turn.
    const rec = asRecord(params);
    await live.rpc.respond(id, {
      scope: "turn",
      permissions: rec?.permissions ?? {},
    });
    return;
  }

  const auto = autoApproval(live.runtimeMode, mapped.kind);
  if (auto) {
    await live.rpc.respond(id, {
      decision: toCodexApprovalDecision(auto, mapped.kind),
    });
    return;
  }

  const pending = waitApproval(live, uiId, id, mapped.kind, threadId);
  live.onEvent(mapped.event);
  const decision = await pending;
  live.onEvent({
    type: "approval.resolved",
    requestId: uiId,
    decision,
  });
  if (decision === "cancelled") return;
  await live.rpc.respond(id, {
    decision: toCodexApprovalDecision(decision, mapped.kind),
  });
}

function waitApproval(
  live: Live,
  uiId: number,
  rpcId: JsonRpcId,
  kind: CodexApprovalKind,
  threadId: string,
): Promise<ApprovalOutcome> {
  return new Promise<ApprovalOutcome>((resolve) => {
    live.approvals.set(uiId, { rpcId, threadId, kind, resolve });
  }).finally(() => {
    live.approvals.delete(uiId);
  });
}

function autoApproval(
  runtimeMode: RuntimeMode,
  kind: CodexApprovalKind,
): ApprovalDecision | null {
  if (runtimeMode === "supervised") return null;
  if (runtimeMode === "full-access") return "allow";
  if (runtimeMode === "auto") {
    // auto_review is set on the server; still prompt if Codex asks.
    return null;
  }
  // auto-accept-edits: auto file changes, ask for commands.
  if (kind === "file-change") return "allow";
  return null;
}

/** Exported for tests. */
export function __codexTestReset(): void {
  liveByThread.clear();
  resumeByThread.clear();
  cancelledThreads.clear();
  cancelEpoch.clear();
  sessionAccount.clear();
  sessionTurns.clear();
  failingOver.clear();
  resetCodexAccounts();
}

export function __codexTestResumeMap(): Map<string, Resume> {
  return resumeByThread;
}

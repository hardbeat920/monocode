import { nativeModelId } from "../../../../features/sessions/model/models";
import { AcpSubagents } from "../../core/acpSubagents";
import type { RuntimeMode } from "../../../../features/sessions/model/session";
import { AcpClient, type AcpHandlers } from "../../core/acp";
import { recoverAcpSession } from "../../core/acpLifecycle";
import {
  killChild,
  resolveAntigravityBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import {
  autoPermissionOption,
  asRecord,
  eventsFromAcpUpdate,
  extractModelConfigId,
  antigravityModeId,
  antigravityPromptBlocks,
  antigravitySpawnCwd,
  permissionOptionId,
  permissionRequestFromAcp,
  readConfigOptions,
  resolveSettingConfigId,
  sessionIdFromResult,
  type SessionConfigOption,
} from "./antigravityProtocol";
import type {
  ApprovalDecision,
  HarnessEvent,
  SendTurnInput,
  SteerTurnInput,
} from "../../core/types";

type SessionSetupResult = {
  sessionId?: string;
  session_id?: string;
  configOptions?: unknown;
};

type Live = {
  threadId: string;
  /** Generation-scoped child id (`thread#n`): a recycled process can never
   *  deliver stdout to or take writes from a different generation. */
  childKey: string;
  subagents: AcpSubagents;
  acp: AcpClient;
  acpSessionId: string;
  cwd: string;
  modelConfigId: string;
  configOptions: SessionConfigOption[];
  muteUpdates: boolean;
  cancelled: boolean;
  /** Server state is unknowable after a mid-prompt cancel; recycle before reuse. */
  stale: boolean;
  runtimeMode: RuntimeMode;
  planning: boolean;
  onEvent: (event: HarnessEvent) => void;
  approvals: Map<number, (decision: ApprovalDecision) => void>;
  promptInFlight: boolean;
  /** True while the owning turn may have state-changing requests in flight —
   *  a cancel here makes the transport indeterminate even mid-config. */
  turnActive: boolean;
  stallNotified: boolean;
  watchdog?: ReturnType<typeof setTimeout>;
};

type Resume = {
  acpSessionId: string;
  cwd: string;
};

// Bound startup and control requests; a prompt may legitimately run much longer.
const INIT_TIMEOUT_MS = 12_000;
const SESSION_TIMEOUT_MS = 45_000;
const CONTROL_TIMEOUT_MS = 15_000;
const PROMPT_TIMEOUT_MS = 30 * 60_000;
// Antigravity answers session/prompt only after its post-turn work (trajectory
// idle + external hooks) finishes, so a wedged server never resolves it. Silence
// is ambiguous — a long quiet tool call is legitimate — so past this point we
// surface a status note instead of killing the turn.
const STALL_NOTIFY_MS = 120_000;

const AUTH_HELP = "Run `agy` once in Terminal to sign in.";

function antigravityError(error: unknown): Error {
  const detail = error instanceof Error ? error.message : String(error);
  if (detail.includes(AUTH_HELP)) return new Error(detail);
  return new Error(/auth|login|sign.in|credential|api.key/i.test(detail)
    ? `${detail.trim()}\n\n${AUTH_HELP}` : detail);
}

function isTimeout(error: unknown): boolean {
  return error instanceof Error && error.message.endsWith("timed out");
}

const CLIENT_CAPABILITIES = {
  fs: { readTextFile: false, writeTextFile: false },
  terminal: false,
  // Advertise config-option support — a compliant agent may omit
  // session.configOptions entirely for clients that never claim it, which
  // would silently drop model selection.
  session: { configOptions: { boolean: {} } },
};

const liveByThread = new Map<string, Live>();
const resumeByThread = new Map<string, Resume>();
// Every user-intent cancel/stop/forget bumps the epoch: sends already pending
// at that moment are suppressed, while a send that arrives later captures the
// new epoch and proceeds normally.
const cancelEpoch = new Map<string, number>();
// Bumped by user-intent invalidation (cancel/stop/forget) — never by internal
// recycles. Lifecycle work validates the epoch captured at submission so
// queued steps cannot resurrect a session the user already abandoned.
const sessionEpoch = new Map<string, number>();
// Startup, teardown, and replacement serialize through this chain per thread:
// concurrent first sends share one startup, and a failed start lets the next
// waiter retry exactly once ownership is rechecked.
const lifecycleByThread = new Map<string, Promise<void>>();
// A startup that has spawned but not yet published its Live owns no entry in
// liveByThread, so cancel/stop would otherwise find nothing to unwind and the
// next send would queue behind the 45s session-request timeout. Registering
// the in-flight setup lets user intent reject its wire calls and kill its
// child immediately.
const pendingSetupByThread = new Map<
  string,
  { acp: AcpClient; childKey: string }
>();
// Turns serialize per thread, not per Live: a recycled transport must not let
// a queued send run concurrently on the replacement.
const turnsByThread = new Map<string, Promise<void>>();
let childSeq = 0;

/** Live Antigravity adapter. Spawns `agy_acp_server.par`, not `agy acp`. */
export async function sendAntigravityTurn(input: SendTurnInput): Promise<void> {
  const epoch = cancelEpoch.get(input.sessionId) ?? 0;
  const cancelled = () => (cancelEpoch.get(input.sessionId) ?? 0) !== epoch;
  try {
    // Eager prewarm only starts a child when none exists — it never tears down
    // a parked transport, so a queued send can never interrupt a running turn.
    await ensureLive(input, false);
  } catch (error) {
    if (cancelled()) return;
    throw error;
  }
  if (cancelled()) return;

  const run = (turnsByThread.get(input.sessionId) ?? Promise.resolve())
    .catch(() => undefined)
    .then(async () => {
      if (cancelled()) return;
      let live: Live;
      try {
        // This send owns the turn: it may recycle a stale or wrong-cwd
        // transport. A superseded step (cancel/stop/forget while queued) is a
        // quiet no-op, not an error.
        live = await ensureLive(input, true);
      } catch (error) {
        if (cancelled()) return;
        throw error;
      }
      if (cancelled()) return;
      // Bind the listener and policy only when this turn actually starts, so a
      // queued send cannot redirect or repolicy the turn still running.
      live.onEvent = input.onEvent;
      live.runtimeMode = input.runtimeMode;
      live.planning = input.intent === "plan";
      live.cancelled = false;
      // From here until the prompt settles, a cancel can interrupt an
      // in-flight state-changing request whose server-side effect is
      // unknowable — mark the transport stale so the next send recycles.
      live.turnActive = true;
      try {
        await applyModelSelection(live, input);
        if (live.cancelled || cancelled()) return;
        await applyRuntimeMode(
          live,
          input.runtimeMode,
          input.intent === "plan",
        );
        // Resume/load history stays muted through setup and configuration.
        // Open the transcript only once the new turn is ready to prompt.
        live.muteUpdates = false;
        if (live.cancelled || cancelled()) return;
        await prompt(live, input);
      } catch (error) {
        if (live.cancelled) return;
        // A timed-out or failed turn leaves the process state unknowable. Keep
        // its provider session id, but recycle this generation's child so the
        // next turn resumes on a fresh transport. Skip if a newer live already
        // replaced it — the teardown below is generation-checked.
        if (liveByThread.get(input.sessionId) === live) {
          await teardownLive(live);
        }
        throw error;
      } finally {
        live.turnActive = false;
      }
    });
  turnsByThread.set(input.sessionId, run);
  try {
    await run;
  } finally {
    if (turnsByThread.get(input.sessionId) === run) {
      turnsByThread.delete(input.sessionId);
    }
  }
}

export async function steerAntigravityTurn(_input: SteerTurnInput): Promise<void> {
  throw new Error("Antigravity does not support steering an in-flight turn");
}

export function respondAntigravityApproval(
  sessionId: string,
  requestId: number,
  decision: ApprovalDecision,
) {
  liveByThread.get(sessionId)?.approvals.get(requestId)?.(decision);
}

export async function cancelAntigravityTurn(sessionId: string): Promise<void> {
  cancelEpoch.set(sessionId, (cancelEpoch.get(sessionId) ?? 0) + 1);
  // Also retire queued/in-flight lifecycle work: a cancel before the child
  // exists must not leave a process spawning in the background.
  sessionEpoch.set(sessionId, (sessionEpoch.get(sessionId) ?? 0) + 1);
  abortPendingSetup(sessionId);
  const live = liveByThread.get(sessionId);
  if (!live) return;
  live.cancelled = true;
  live.muteUpdates = true;
  // A prompt cancelled on the wire may still be running server-side, and any
  // control request in flight may already have taken effect — either way this
  // transport's state is indeterminate, so the next send must not reuse it.
  if (live.turnActive) live.stale = true;
  for (const resolve of live.approvals.values()) resolve("deny");
  live.approvals.clear();
  // Unwind the local turn first: a blocked stdin must not keep the UI's send
  // promise (or this cancel) wedged. The wire notify is best-effort.
  live.acp.rejectPending(new Error("cancelled"));
  void live.acp
    .notify("session/cancel", { sessionId: live.acpSessionId })
    .catch(() => undefined);
}

/**
 * Tear down exactly this generation: settle its approvals, close its client,
 * and kill only its child. Internal recycles go through here — they must NOT
 * bump the user-intent epochs or they would suppress unrelated queued sends.
 */
async function teardownLive(live: Live): Promise<void> {
  if (liveByThread.get(live.threadId) === live) {
    liveByThread.delete(live.threadId);
  }
  settleLive(live);
  live.acp.close();
  unwatchChild(live.childKey);
  await killChild(live.childKey).catch(() => undefined);
}

export async function stopAntigravitySession(sessionId: string): Promise<void> {
  // Stopping the session invalidates work already queued for it, not just the
  // registered live — a send waiting its turn must not resurrect it.
  cancelEpoch.set(sessionId, (cancelEpoch.get(sessionId) ?? 0) + 1);
  sessionEpoch.set(sessionId, (sessionEpoch.get(sessionId) ?? 0) + 1);
  abortPendingSetup(sessionId);
  const live = liveByThread.get(sessionId);
  if (live) await teardownLive(live);
}

/**
 * A setup still inside initialize/session requests owns no Live yet — reject
 * its pending wire calls and kill its child so a cancel or stop lands
 * immediately instead of surfacing after the session-request timeout.
 */
function abortPendingSetup(sessionId: string): void {
  const pending = pendingSetupByThread.get(sessionId);
  if (!pending) return;
  // close() both rejects in-flight requests and makes any further request on
  // this client fail immediately — a cancelled setup cannot fall through the
  // resume/load/new ladder into another 45s wait.
  pending.acp.close(new Error("cancelled"));
  void killChild(pending.childKey).catch(() => undefined);
}

export async function forgetAntigravitySession(sessionId: string): Promise<void> {
  resumeByThread.delete(sessionId);
  await stopAntigravitySession(sessionId);
}

export function bindAntigravitySession(
  threadId: string,
  acpSessionId: string,
  cwd: string,
): void {
  const sessionId = acpSessionId.trim();
  if (!threadId || !sessionId || !cwd.trim()) return;
  resumeByThread.set(threadId, { acpSessionId: sessionId, cwd });
}

/** Terminal settle for a retired transport: stop the clock and release waits. */
function settleLive(live: Live): void {
  live.cancelled = true;
  live.muteUpdates = true;
  live.promptInFlight = false;
  if (live.watchdog) {
    clearTimeout(live.watchdog);
    live.watchdog = undefined;
  }
  for (const resolve of live.approvals.values()) resolve("deny");
  live.approvals.clear();
}

/**
 * Server silence is ambiguous — a quiet tool call and a wedged post-turn hook
 * look identical on the wire. Past STALL_NOTIFY_MS we only surface a status so
 * the user knows a stop-and-resend is the recovery path; nothing is killed.
 * Re-armed by any inbound traffic and after an approval resolves.
 */
function noteActivity(live: Live): void {
  live.stallNotified = false;
  if (!live.promptInFlight) return;
  if (live.watchdog) clearTimeout(live.watchdog);
  live.watchdog = setTimeout(() => {
    live.watchdog = undefined;
    if (liveByThread.get(live.threadId) !== live) return;
    if (!live.promptInFlight || live.approvals.size > 0) return;
    if (live.stallNotified) return;
    live.stallNotified = true;
    live.onEvent({
      type: "status",
      text: "Antigravity has been quiet for two minutes — its post-turn work may be stuck. Stop and resend to recover.",
    });
  }, STALL_NOTIFY_MS);
}

async function ensureLive(input: SendTurnInput, recycle: boolean): Promise<Live> {
  const existing = liveByThread.get(input.sessionId);
  if (existing) {
    if (!recycle) return existing;
    if (!existing.stale && existing.cwd === input.cwd) return existing;
  }
  return queueLifecycle(input, recycle);
}

/**
 * One lifecycle operation at a time per thread: teardown of a stale or
 * wrong-cwd live and the next startup run inside the same chain, so a queued
 * send can never overlap a recycle or a failed attempt's cleanup. The session
 * epoch captured at submission bounds the whole step — a cancel, stop, or
 * forget landing mid-step retires it instead of letting it publish a session
 * the user already abandoned.
 */
function queueLifecycle(input: SendTurnInput, recycle: boolean): Promise<Live> {
  const life = sessionEpoch.get(input.sessionId) ?? 0;
  const prev = lifecycleByThread.get(input.sessionId) ?? Promise.resolve();
  const next = prev.then(async () => {
    if ((sessionEpoch.get(input.sessionId) ?? 0) !== life) {
      throw new Error("Antigravity session superseded");
    }
    let live = liveByThread.get(input.sessionId);
    if (live) {
      if (!recycle) return live;
      if (!live.stale && live.cwd === input.cwd) return live;
      if (live.cwd !== input.cwd) resumeByThread.delete(input.sessionId);
      await teardownLive(live);
      live = undefined;
    }
    return startLive(input, life);
  });
  lifecycleByThread.set(
    input.sessionId,
    next.then(
      () => undefined,
      () => undefined,
    ),
  );
  return next;
}

async function startLive(input: SendTurnInput, life: number): Promise<Live> {
  const resume = resumeByThread.get(input.sessionId);
  const canLoad = resume != null && resume.cwd === input.cwd;
  if (resume && resume.cwd !== input.cwd) {
    resumeByThread.delete(input.sessionId);
  }

  const retired = () => (sessionEpoch.get(input.sessionId) ?? 0) !== life;
  if (retired()) throw new Error("Antigravity session stopped during startup");
  const childKey = `${input.sessionId}#${childSeq++}`;

  const { path, args } = await resolveAntigravityBinary();
  const handlers: AcpHandlers = {};
  const acp = new AcpClient(childKey, handlers);
  const pendingSetup = { acp, childKey };
  pendingSetupByThread.set(input.sessionId, pendingSetup);
  const clearPending = () => {
    if (pendingSetupByThread.get(input.sessionId) === pendingSetup) {
      pendingSetupByThread.delete(input.sessionId);
    }
  };
  const liveRef: { current: Live | null } = { current: null };

  handlers.onNotification = (method, params) => {
    const live = liveRef.current;
    if (!live) return;
    noteActivity(live);
    handleNotification(live, method, params);
  };
  handlers.onRequestRaw = (id, method) => {
    if (typeof id === "number") return;
    void acp.respondError(id, {
      code: -32601,
      message: `Method not found: ${method}`,
    }).catch(() => undefined);
  };
  handlers.onRequest = (id, method, params) => {
    const live = liveRef.current;
    if (live) noteActivity(live);
    if (!live) {
      void acp
        .respondError(id, {
          code: -32601,
          message: `Method not found: ${method}`,
        })
        .catch(() => undefined);
      return;
    }
    void handleRequest(live, id, method, params).catch((error) => {
      // A reply that could not be written is a transport failure: the provider
      // is still waiting for an answer that will never arrive. Fail the
      // generation so the owning turn unwinds instead of hanging on the prompt
      // deadline. Already-retired or cancelled generations stay quiet.
      if (liveByThread.get(live.threadId) !== live || live.cancelled) return;
      live.stale = true;
      acp.close(error instanceof Error ? error : new Error(String(error)));
    });
  };

  // startLive runs once per session, so these handlers outlive the turn that
  // created them. Routing through the live record keeps them on the *current*
  // turn's listener — capturing `input.onEvent` meant every exit and stderr
  // error after turn 1 was addressed to a finished turn and silently dropped,
  // leaving the session spinning on "Working…" forever.
  const emit = (event: HarnessEvent) => {
    (liveRef.current?.onEvent ?? input.onEvent)(event);
  };

  watchChild(
    childKey,
    (line) => acp.pushLine(line),
    (code) => {
      unwatchChild(childKey);
      const live = liveRef.current;
      if (liveByThread.get(input.sessionId) === live) {
        liveByThread.delete(input.sessionId);
      }
      // Settle before closing so in-flight requests unwind as cancelled rather
      // than surfacing a transport error for a dead process.
      if (live) settleLive(live);
      acp.close(new Error("Antigravity exited"));
      // A generation retired by user intent settles quietly — no session.ended
      // for a setup the user already cancelled or stopped.
      if (live || !retired()) emit({ type: "session.ended", code });
    },
    (line) => {
      console.debug("[monocode] antigravity stderr", line);
    },
  );

  try {
    // Binary resolution awaited above may have raced a stop/forget — re-check
    // before the child is ever spawned, not just after.
    if (retired()) throw new Error("Antigravity session stopped during startup");
    await spawnChild(childKey, path, args, antigravitySpawnCwd(path, input.cwd));
    if (retired()) throw new Error("Antigravity session stopped during startup");
    try {
      await acp.request(
        "initialize",
        {
          protocolVersion: 1,
          clientCapabilities: CLIENT_CAPABILITIES,
          clientInfo: { name: "monocode", version: "0.1.0" },
        },
        INIT_TIMEOUT_MS,
      );
    } catch (error) {
      throw antigravityError(error);
    }

    const previous = canLoad && resume ? resume.acpSessionId : undefined;
    const recovery = await recoverAcpSession(previous, {
      resume: async (sessionId) => {
        if (retired()) throw new Error("Antigravity session stopped during startup");
        return acp.request<SessionSetupResult>(
          "session/resume",
          { sessionId, cwd: input.cwd, mcpServers: [] },
          SESSION_TIMEOUT_MS,
        );
      },
      load: async (sessionId) => {
        if (retired()) throw new Error("Antigravity session stopped during startup");
        return acp.request<SessionSetupResult>(
          "session/load",
          { sessionId, cwd: input.cwd, mcpServers: [] },
          SESSION_TIMEOUT_MS,
        );
      },
      create: async () => {
        if (retired()) throw new Error("Antigravity session stopped during startup");
        return acp.request<SessionSetupResult>(
          "session/new",
          { cwd: input.cwd, mcpServers: [] },
          SESSION_TIMEOUT_MS,
        );
      },
      sessionId: sessionIdFromResult,
      isTimeout,
    });
    const setup = recovery.setup;
    const acpSessionId = recovery.sessionId;
    const didLoad = recovery.restored;
    if (retired()) throw new Error("Antigravity session stopped during startup");
    if (!recovery.restored && previous) {
      emit({
        type: "status",
        text: "Antigravity could not restore the previous conversation � starting a new session.",
      });
    }

    if (retired()) throw new Error("Antigravity session stopped during startup");

    const configOptions = readConfigOptions(setup?.configOptions);
    const live: Live = {
      threadId: input.sessionId,
      childKey,
      subagents: new AcpSubagents(),
      acp,
      acpSessionId,
      cwd: input.cwd,
      modelConfigId: extractModelConfigId(configOptions),
      configOptions,
      muteUpdates: didLoad,
      cancelled: false,
      stale: false,
      runtimeMode: input.runtimeMode,
      planning: input.intent === "plan",
      onEvent: input.onEvent,
      approvals: new Map(),
      promptInFlight: false,
      turnActive: false,
      stallNotified: false,
    };
    liveRef.current = live;
    liveByThread.set(input.sessionId, live);
    resumeByThread.set(input.sessionId, {
      acpSessionId,
      cwd: input.cwd,
    });
    // Registration ends before the first listener callback: a cancel reentered
    // from onEvent must take the live path, never abortPendingSetup — closing
    // this client now would orphan the just-published transport.
    clearPending();
    live.onEvent({
      type: "session.providerBound",
      providerSessionId: acpSessionId,
    });
    live.onEvent({ type: "session.started" });
    return live;
  } catch (error) {
    // Clean up only this generation's child — a successor may already own the
    // thread slot, so nothing here may touch the thread-keyed maps. A listener
    // that threw post-publish is the exception: drop the dead transport this
    // generation registered so the next send cannot reuse it.
    acp.close(error instanceof Error ? error : new Error(String(error)));
    unwatchChild(childKey);
    await killChild(childKey).catch(() => undefined);
    if (
      liveRef.current != null &&
      liveByThread.get(input.sessionId) === liveRef.current
    ) {
      liveByThread.delete(input.sessionId);
    }
    throw antigravityError(error);
  } finally {
    clearPending();
  }
}

async function applyModelSelection(
  live: Live,
  input: SendTurnInput,
): Promise<void> {
  const base = nativeModelId(input.model);
  const settings = input.modelSettings ?? {};
  const modelConfigId =
    live.modelConfigId === "provider" ? "model" : live.modelConfigId;

  await setConfigOption(live, modelConfigId, base);

  for (const [settingId, value] of Object.entries(settings)) {
    const configId = resolveSettingConfigId(live.configOptions, settingId);
    if (!configId || configId === "provider") continue;
    await setConfigOption(live, configId, value);
  }
}

async function applyRuntimeMode(
  live: Live,
  runtimeMode: RuntimeMode,
  planning = false,
): Promise<void> {
  // Fail closed: a rejected downgrade must not leave a prior yolo mode active.
  await live.acp
    .request(
      "session/set_mode",
      {
        sessionId: live.acpSessionId,
        modeId: antigravityModeId(runtimeMode, planning),
      },
      CONTROL_TIMEOUT_MS,
    );
}

async function setConfigOption(
  live: Live,
  configId: string,
  value: string | boolean,
): Promise<void> {
  const current = live.configOptions.find((option) => option.id === configId);
  // session/set_config_option is only valid for options the session itself
  // advertised via configOptions — sending an unadvertised id is a protocol
  // violation the provider may reject or ignore unpredictably.
  if (!current) return;
  // Boolean options take the typed {type:"boolean", value:boolean} variant;
  // everything else is a value-id string (the wire default when type absent).
  const isBool = current.type === "boolean";
  if (
    isBool &&
    typeof value !== "boolean" &&
    value !== "true" &&
    value !== "false"
  ) {
    // A boolean option can only carry a boolean — a stray string (e.g. a model
    // id that resolved here by accident) must not be coerced into `false`.
    return;
  }
  const boolValue = value === true || value === "true";
  const already = isBool
    ? current.currentValue === boolValue
    : String(current.currentValue ?? "") === String(value);
  if (already) return;

  const params: Record<string, unknown> = {
    sessionId: live.acpSessionId,
    configId,
    value: isBool ? boolValue : String(value),
  };
  if (isBool) params.type = "boolean";
  const result = await live.acp.request<SessionSetupResult>(
    "session/set_config_option",
    params,
    CONTROL_TIMEOUT_MS,
  );
  if (Array.isArray(result?.configOptions)) {
    live.configOptions = readConfigOptions(result.configOptions);
    live.modelConfigId = extractModelConfigId(live.configOptions);
  }
}

async function prompt(live: Live, input: SendTurnInput): Promise<void> {
  try {
    const blocks = antigravityPromptBlocks(input.text, input.attachments);
    if (blocks.length === 0) return;
    live.promptInFlight = true;
    noteActivity(live);
    const result = await live.acp.request(
      "session/prompt",
      {
        sessionId: live.acpSessionId,
        prompt: blocks,
      },
      PROMPT_TIMEOUT_MS,
    );
    const stopReason = asRecord(result)?.stopReason;
    if (live.cancelled || stopReason === "cancelled") return;
    // A fulfilled request is not necessarily a normal end: refusals,
    // truncations, and provider-specific reasons all surface as errors.
    if (stopReason != null && stopReason !== "end_turn") {
      live.onEvent({
        type: "session.error",
        message: `Antigravity ended the turn (${String(stopReason)}).`,
      });
      return;
    }
    live.onEvent({ type: "message.completed" });
    live.onEvent({ type: "reasoning.completed" });
  } catch (error) {
    if (live.cancelled) return;
    const failure = antigravityError(error);
    live.onEvent({
      type: "session.error",
      message: failure.message,
    });
    throw failure;
  } finally {
    live.promptInFlight = false;
    if (live.watchdog) {
      clearTimeout(live.watchdog);
      live.watchdog = undefined;
    }
  }
}

function handleNotification(live: Live, method: string, params: unknown) {
  if (method !== "session/update") return;
  const rec = asRecord(params);
  const update = asRecord(rec?.update) ?? rec;
  // Config state stays fresh even while the transcript is muted — a stale
  // currentValue would resend set_config_option for a value already applied.
  if (
    update?.sessionUpdate === "config_option_update" &&
    Array.isArray(update.configOptions)
  ) {
    live.configOptions = readConfigOptions(update.configOptions);
    live.modelConfigId = extractModelConfigId(live.configOptions);
  }
  if (live.muteUpdates) return;
  for (const event of live.subagents.route(params, eventsFromAcpUpdate(params))) {
    live.onEvent(event);
  }
}

async function handleRequest(
  live: Live,
  id: number,
  method: string,
  params: unknown,
) {
  if (method === "session/request_permission") {
    await handlePermission(live, id, params);
    return;
  }
  // Propagate: the caller turns a failed response write into a transport
  // failure for the active generation, so the provider cannot wedge waiting
  // for a reply that never left the pipe.
  await live.acp.respondError(id, {
    code: -32601,
    message: `Method not found: ${method}`,
  });
}

async function handlePermission(live: Live, id: number, params: unknown) {
  const request = permissionRequestFromAcp(params);
  // Protocol replies always flow; UI events are gated on the turn still being
  // live so a cancelled run cannot leave approval cards behind.
  if (
    request.callId &&
    live.promptInFlight &&
    !live.cancelled &&
    !live.muteUpdates
  ) {
    live.onEvent({
      type: "tool.updated",
      callId: request.callId,
      title: request.title,
      kind: request.kind,
      preview: request.preview,
    });
  }
  let optionId: string | null;
  if (
    !live.promptInFlight ||
    live.cancelled ||
    live.muteUpdates ||
    request.optionIds.length === 0
  ) {
    optionId = null;
  } else if (live.planning) {
    optionId = permissionOptionId(
      request.kind === "read" || request.kind === "search" ? "allow" : "deny",
      request.optionIds,
      request.optionKinds,
    );
  } else {
    optionId = autoPermissionOption(
      live.runtimeMode, request.kind, request.optionIds, request.optionKinds,
    );
    if (!optionId) {
      const pending = new Promise<ApprovalDecision>((resolve) => {
        live.approvals.set(id, resolve);
      });
      live.onEvent({
        type: "approval.requested",
        requestId: id,
        title: request.title,
        kind: request.kind,
        callId: request.callId,
        preview: request.preview,
      });
      const decision = await pending;
      live.approvals.delete(id);
      // The approval wait itself was the silence; restart the stall clock.
      noteActivity(live);
      // Always close the card — even a cancelled/exited turn must not leave a
      // pending approval rendered as actionable.
      live.onEvent({ type: "approval.resolved", requestId: id, decision });
      optionId = live.cancelled ? null : permissionOptionId(
        decision, request.optionIds, request.optionKinds,
      );
    }
  }
  await live.acp.respond(id, {
    outcome: optionId
      ? { outcome: "selected", optionId }
      : { outcome: "cancelled" },
  });
}

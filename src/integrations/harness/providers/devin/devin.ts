import { nativeModelId } from "../../../../features/sessions/model/models";
import type { RuntimeMode } from "../../../../features/sessions/model/session";
import { AcpClient, type AcpHandlers } from "../../core/acp";
import { AcpSubagents } from "../../core/acpSubagents";
import {
  killChild,
  resolveDevinBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import type {
  ApprovalDecision,
  HarnessEvent,
  SendTurnInput,
  SteerTurnInput,
} from "../../core/types";
import {
  DEVIN_CLIENT_CAPABILITIES,
  DEVIN_CLIENT_INFO,
  asRecord,
  autoPermissionOption,
  devinModeId,
  devinPromptBlocks,
  eventsFromAcpUpdate,
  extractModelConfigId,
  permissionOptionId,
  permissionRequestFromAcp,
  readConfigOptions,
  resolveSettingConfigId,
  sessionIdFromResult,
  type SessionConfigOption,
} from "./devinProtocol";

type SessionSetupResult = {
  sessionId?: string;
  session_id?: string;
  configOptions?: unknown;
};

type Resume = {
  acpSessionId: string;
  cwd: string;
};

type Live = {
  threadId: string;
  childKey: string;
  acp: AcpClient;
  subagents: AcpSubagents;
  acpSessionId: string;
  cwd: string;
  configOptions: SessionConfigOption[];
  modelConfigId: string;
  onEvent: (event: HarnessEvent) => void;
  runtimeMode: RuntimeMode;
  planning: boolean;
  cancelled: boolean;
  stale: boolean;
  promptInFlight: boolean;
  turnActive: boolean;
  approvals: Map<number, (decision: ApprovalDecision) => void>;
};

const INIT_TIMEOUT_MS = 20_000;
const SESSION_TIMEOUT_MS = 45_000;
const CONTROL_TIMEOUT_MS = 20_000;
const PROMPT_TIMEOUT_MS = 30 * 60_000;
const AUTH_HELP = "Run `devin auth login` in Terminal, then retry.";

const liveByThread = new Map<string, Live>();
const resumeByThread = new Map<string, Resume>();
const cancelEpoch = new Map<string, number>();
const sessionEpoch = new Map<string, number>();
const lifecycleByThread = new Map<string, Promise<void>>();
const pendingSetupByThread = new Map<
  string,
  { acp: AcpClient; childKey: string }
>();
const turnsByThread = new Map<string, Promise<void>>();
let childSeq = 0;

function devinError(error: unknown): Error {
  const detail = error instanceof Error ? error.message : String(error);
  if (/auth|login|sign.?in|credential|unauthorized|401/i.test(detail)) {
    return new Error(`${detail.trim()}\n\n${AUTH_HELP}`);
  }
  return error instanceof Error ? error : new Error(detail);
}

function isTimeout(error: unknown): boolean {
  return error instanceof Error && /timed out$/i.test(error.message);
}

export async function sendDevinTurn(input: SendTurnInput): Promise<void> {
  const epoch = cancelEpoch.get(input.sessionId) ?? 0;
  const cancelled = () => (cancelEpoch.get(input.sessionId) ?? 0) !== epoch;

  try {
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
        live = await ensureLive(input, true);
      } catch (error) {
        if (cancelled()) return;
        throw error;
      }
      if (cancelled()) return;

      live.onEvent = input.onEvent;
      live.runtimeMode = input.runtimeMode;
      live.planning = input.intent === "plan";
      live.cancelled = false;
      live.turnActive = true;

      try {
        await applyModelSelection(live, input);
        if (live.cancelled || cancelled()) return;
        await applyRuntimeMode(live, input.runtimeMode, live.planning);
        if (live.cancelled || cancelled()) return;
        input.onAccepted?.();
        await prompt(live, input);
      } catch (error) {
        if (live.cancelled || cancelled()) return;
        if (liveByThread.get(input.sessionId) === live) {
          await teardownLive(live);
        }
        throw devinError(error);
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

export async function steerDevinTurn(_input: SteerTurnInput): Promise<void> {
  throw new Error("Devin does not support steering an in-flight ACP turn");
}

export function respondDevinApproval(
  sessionId: string,
  requestId: number,
  decision: ApprovalDecision,
): void {
  liveByThread.get(sessionId)?.approvals.get(requestId)?.(decision);
}

export async function cancelDevinTurn(sessionId: string): Promise<void> {
  cancelEpoch.set(sessionId, (cancelEpoch.get(sessionId) ?? 0) + 1);
  sessionEpoch.set(sessionId, (sessionEpoch.get(sessionId) ?? 0) + 1);
  abortPendingSetup(sessionId);

  const live = liveByThread.get(sessionId);
  if (!live) return;
  live.cancelled = true;
  if (live.turnActive) live.stale = true;
  settleApprovals(live);
  live.acp.rejectPending(new Error("cancelled"));
  void live.acp
    .notify("session/cancel", { sessionId: live.acpSessionId })
    .catch(() => undefined);
}

export async function stopDevinSession(sessionId: string): Promise<void> {
  cancelEpoch.set(sessionId, (cancelEpoch.get(sessionId) ?? 0) + 1);
  sessionEpoch.set(sessionId, (sessionEpoch.get(sessionId) ?? 0) + 1);
  abortPendingSetup(sessionId);
  const live = liveByThread.get(sessionId);
  if (live) await teardownLive(live);
}

export async function forgetDevinSession(sessionId: string): Promise<void> {
  resumeByThread.delete(sessionId);
  await stopDevinSession(sessionId);
}

export function bindDevinSession(
  threadId: string,
  acpSessionId: string,
  cwd: string,
): void {
  const providerId = acpSessionId.trim();
  if (!threadId || !providerId || !cwd.trim()) return;
  resumeByThread.set(threadId, { acpSessionId: providerId, cwd });
}

function abortPendingSetup(sessionId: string): void {
  const pending = pendingSetupByThread.get(sessionId);
  if (!pending) return;
  pending.acp.close(new Error("cancelled"));
  void killChild(pending.childKey).catch(() => undefined);
}

async function ensureLive(input: SendTurnInput, recycle: boolean): Promise<Live> {
  const existing = liveByThread.get(input.sessionId);
  if (existing) {
    if (!recycle) return existing;
    if (!existing.stale && existing.cwd === input.cwd) return existing;
  }
  return queueLifecycle(input, recycle);
}

function queueLifecycle(input: SendTurnInput, recycle: boolean): Promise<Live> {
  const life = sessionEpoch.get(input.sessionId) ?? 0;
  const previous = lifecycleByThread.get(input.sessionId) ?? Promise.resolve();
  const next = previous.then(async () => {
    if ((sessionEpoch.get(input.sessionId) ?? 0) !== life) {
      throw new Error("Devin session superseded");
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
  if (retired()) throw new Error("Devin session stopped during startup");

  const childKey = `${input.sessionId}#devin-${childSeq++}`;
  const { path } = await resolveDevinBinary();
  if (retired()) throw new Error("Devin session stopped during startup");

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
    handleNotification(live, method, params);
  };
  handlers.onRequest = (id, method, params) => {
    const live = liveRef.current;
    void handleRequest(live, acp, id, method, params).catch((error) => {
      if (!live || live.cancelled) return;
      live.stale = true;
      acp.close(error instanceof Error ? error : new Error(String(error)));
    });
  };

  const emit = (event: HarnessEvent) => {
    (liveRef.current?.onEvent ?? input.onEvent)(event);
  };

  watchChild(
    childKey,
    (line) => acp.pushLine(line),
    (code) => {
      unwatchChild(childKey);
      const live = liveRef.current;
      if (live && liveByThread.get(input.sessionId) === live) {
        liveByThread.delete(input.sessionId);
      }
      if (live) settleLive(live);
      acp.close(new Error("Devin exited"));
      if (live || !retired()) emit({ type: "session.ended", code });
    },
    (line) => {
      if (line.trim()) console.debug("[monocode] devin stderr", line);
    },
  );

  try {
    if (retired()) throw new Error("Devin session stopped during startup");
    await spawnChild(childKey, path, ["acp"], input.cwd);
    if (retired()) throw new Error("Devin session stopped during startup");

    await acp.request(
      "initialize",
      {
        protocolVersion: 1,
        clientCapabilities: DEVIN_CLIENT_CAPABILITIES,
        clientInfo: DEVIN_CLIENT_INFO,
      },
      INIT_TIMEOUT_MS,
    );
    if (retired()) throw new Error("Devin session stopped during startup");

    let setup: SessionSetupResult | undefined;
    let acpSessionId: string | undefined;
    let loadFailed = false;

    if (canLoad && resume) {
      try {
        setup = await acp.request<SessionSetupResult>(
          "session/load",
          {
            sessionId: resume.acpSessionId,
            cwd: input.cwd,
            mcpServers: [],
          },
          SESSION_TIMEOUT_MS,
        );
        acpSessionId = sessionIdFromResult(setup) ?? resume.acpSessionId;
      } catch (error) {
        if (retired()) throw new Error("Devin session stopped during startup");
        if (isTimeout(error)) throw error;
        loadFailed = true;
        setup = undefined;
        acpSessionId = undefined;
      }
    }

    if (!acpSessionId) {
      if (retired()) throw new Error("Devin session stopped during startup");
      setup = await acp.request<SessionSetupResult>(
        "session/new",
        { cwd: input.cwd, mcpServers: [] },
        SESSION_TIMEOUT_MS,
      );
      acpSessionId = sessionIdFromResult(setup);
      if (acpSessionId && loadFailed) {
        emit({
          type: "status",
          text: "Devin could not restore the previous conversation - starting a new session.",
        });
      }
    }

    if (!acpSessionId) throw new Error("Devin did not return a session id");
    if (retired()) throw new Error("Devin session stopped during startup");

    const configOptions = readConfigOptions(setup?.configOptions);
    const live: Live = {
      threadId: input.sessionId,
      childKey,
      acp,
      subagents: new AcpSubagents(),
      acpSessionId,
      cwd: input.cwd,
      configOptions,
      modelConfigId: extractModelConfigId(configOptions),
      onEvent: input.onEvent,
      runtimeMode: input.runtimeMode,
      planning: input.intent === "plan",
      cancelled: false,
      stale: false,
      promptInFlight: false,
      turnActive: false,
      approvals: new Map(),
    };
    liveRef.current = live;
    liveByThread.set(input.sessionId, live);
    resumeByThread.set(input.sessionId, { acpSessionId, cwd: input.cwd });
    clearPending();
    live.onEvent({
      type: "session.providerBound",
      providerSessionId: acpSessionId,
    });
    live.onEvent({ type: "session.started" });
    return live;
  } catch (error) {
    acp.close(error instanceof Error ? error : new Error(String(error)));
    unwatchChild(childKey);
    await killChild(childKey).catch(() => undefined);
    if (
      liveRef.current != null &&
      liveByThread.get(input.sessionId) === liveRef.current
    ) {
      liveByThread.delete(input.sessionId);
    }
    throw devinError(error);
  } finally {
    clearPending();
  }
}

async function teardownLive(live: Live): Promise<void> {
  if (liveByThread.get(live.threadId) === live) {
    liveByThread.delete(live.threadId);
  }
  settleLive(live);
  live.acp.close();
  unwatchChild(live.childKey);
  await killChild(live.childKey).catch(() => undefined);
}

function settleApprovals(live: Live): void {
  for (const resolve of live.approvals.values()) resolve("deny");
  live.approvals.clear();
}

function settleLive(live: Live): void {
  live.cancelled = true;
  live.promptInFlight = false;
  settleApprovals(live);
}

async function applyModelSelection(
  live: Live,
  input: SendTurnInput,
): Promise<void> {
  const base = nativeModelId(input.model);
  if (base) {
    await setConfigOption(live, live.modelConfigId, base);
  }

  for (const [settingId, value] of Object.entries(input.modelSettings ?? {})) {
    const configId = resolveSettingConfigId(live.configOptions, settingId);
    if (!configId || configId === "provider") continue;
    await setConfigOption(live, configId, value);
  }
}

async function applyRuntimeMode(
  live: Live,
  runtimeMode: RuntimeMode,
  planning: boolean,
): Promise<void> {
  const modeOption = live.configOptions.find(
    (option) =>
      option.id === "mode" ||
      option.category === "mode" ||
      option.id === "permission_mode",
  );
  if (!modeOption) return;
  await setConfigOption(live, modeOption.id, devinModeId(runtimeMode, planning));
}

async function setConfigOption(
  live: Live,
  configId: string,
  value: string | boolean,
): Promise<void> {
  const current = live.configOptions.find((option) => option.id === configId);
  if (!current) return;

  const isBool = current.type === "boolean";
  if (
    isBool &&
    typeof value !== "boolean" &&
    value !== "true" &&
    value !== "false"
  ) {
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
  const blocks = devinPromptBlocks(input.text, input.attachments);
  if (blocks.length === 0) return;

  live.promptInFlight = true;
  try {
    const result = await live.acp.request(
      "session/prompt",
      { sessionId: live.acpSessionId, prompt: blocks },
      PROMPT_TIMEOUT_MS,
    );
    if (live.cancelled) return;
    const stopReason = asRecord(result)?.stopReason;
    if (stopReason != null && stopReason !== "end_turn") {
      if (stopReason === "cancelled") return;
      live.onEvent({
        type: "session.error",
        message: `Devin ended the turn (${String(stopReason)}).`,
      });
      return;
    }
    live.onEvent({ type: "message.completed" });
    live.onEvent({ type: "reasoning.completed" });
  } catch (error) {
    if (live.cancelled) return;
    const failure = devinError(error);
    live.onEvent({ type: "session.error", message: failure.message });
    throw failure;
  } finally {
    live.promptInFlight = false;
  }
}

function handleNotification(
  live: Live,
  method: string,
  params: unknown,
): void {
  if (method !== "session/update") return;

  const rec = asRecord(params);
  const update = asRecord(rec?.update) ?? rec;
  if (
    update?.sessionUpdate === "config_option_update" &&
    Array.isArray(update.configOptions)
  ) {
    live.configOptions = readConfigOptions(update.configOptions);
    live.modelConfigId = extractModelConfigId(live.configOptions);
  }

  if (live.cancelled) return;
  for (const event of live.subagents.route(params, eventsFromAcpUpdate(params))) {
    live.onEvent(event);
  }
}

async function handleRequest(
  live: Live | null,
  acp: AcpClient,
  id: number,
  method: string,
  params: unknown,
): Promise<void> {
  if (method === "_cognition.ai/request_diagnostics") {
    await acp.respond(id, {});
    return;
  }
  if (method === "session/request_permission") {
    if (live) {
      await handlePermission(live, id, params);
    } else {
      await acp.respond(id, { outcome: { outcome: "cancelled" } });
    }
    return;
  }
  await acp.respondError(id, {
    code: -32601,
    message: `Method not found: ${method}`,
  });
}

async function handlePermission(
  live: Live,
  id: number,
  params: unknown,
): Promise<void> {
  const request = permissionRequestFromAcp(params);
  const active = () =>
    liveByThread.get(live.threadId) === live &&
    live.promptInFlight &&
    !live.cancelled;

  if (request.callId && active()) {
    live.onEvent({
      type: "tool.updated",
      callId: request.callId,
      title: request.title,
      kind: request.kind,
      preview: request.preview,
    });
  }

  let optionId: string | null = null;
  if (active() && request.optionIds.length > 0) {
    if (live.planning) {
      optionId = permissionOptionId(
        request.kind === "read" || request.kind === "search" ? "allow" : "deny",
        request.optionIds,
        request.optionKinds,
      );
    } else {
      optionId = autoPermissionOption(
        live.runtimeMode,
        request.kind,
        request.optionIds,
        request.optionKinds,
      );
      if (!optionId) {
        const decision = await new Promise<ApprovalDecision>((resolve) => {
          live.approvals.set(id, resolve);
          live.onEvent({
            type: "approval.requested",
            requestId: id,
            title: request.title,
            kind: request.kind,
            callId: request.callId,
            preview: request.preview,
          });
        });
        live.approvals.delete(id);
        live.onEvent({
          type: "approval.resolved",
          requestId: id,
          decision,
        });
        if (active()) {
          optionId = permissionOptionId(
            decision,
            request.optionIds,
            request.optionKinds,
          );
        }
      }
    }
  }

  await live.acp.respond(id, {
    outcome: optionId
      ? { outcome: "selected", optionId }
      : { outcome: "cancelled" },
  });
}
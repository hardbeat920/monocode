import { promptBlocks } from "../../../../features/sessions/model/attachments";
import { nativeModelId } from "../../../../features/sessions/model/models";
import type { RuntimeMode } from "../../../../features/sessions/model/session";
import { AcpClient, type AcpHandlers } from "../../core/acp";
import { AcpSubagents } from "../../core/acpSubagents";
import { isHarnessAuthError } from "../../core/authSupport";
import {
  killChild,
  resolveCopilotBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import {
  eventsFromAcpUpdate,
  permissionOptionId,
  permissionRequestFromAcp,
  pickAutoOption,
  type GrokPermissionRequest,
} from "../grok/grokProtocol";
import {
  COPILOT_AUTH_HELP,
  copilotCurrentModelId,
  copilotMetricsFromPromptResult,
  copilotModeId,
  copilotSessionId,
  copilotStartupError,
  copilotStderrAuthError,
} from "./copilotProtocol";
import type {
  ApprovalDecision,
  HarnessEvent,
  HarnessSessionInput,
  SendTurnInput,
  SteerTurnInput,
} from "../../core/types";

type Live = {
  subagents: AcpSubagents;
  acp: AcpClient;
  acpSessionId: string;
  cwd: string;
  modelId: string;
  modeId: string;
  muteUpdates: boolean;
  cancelled: boolean;
  runtimeMode: RuntimeMode;
  planning: boolean;
  onEvent: (event: HarnessEvent) => void;
  approvals: Map<number, (decision: ApprovalDecision) => void>;
  turns: Promise<void>;
  activePrompt: Promise<unknown> | null;
  wakePrompt: (() => void) | null;
};

type Resume = { acpSessionId: string; cwd: string };

const INIT_TIMEOUT_MS = 20_000;
const SESSION_TIMEOUT_MS = 45_000;
const CONTROL_TIMEOUT_MS = 20_000;
const PROMPT_TIMEOUT_MS = 30 * 60_000;
const CLIENT_CAPABILITIES = {
  fs: { readTextFile: false, writeTextFile: false },
  terminal: false,
};

const liveByThread = new Map<string, Live>();
const resumeByThread = new Map<string, Resume>();
const cancelledThreads = new Set<string>();

export async function sendCopilotTurn(input: SendTurnInput): Promise<void> {
  let live: Live;
  try {
    live = await ensureLive(input);
  } catch (error) {
    cancelledThreads.delete(input.sessionId);
    throw error;
  }
  if (cancelledThreads.delete(input.sessionId)) {
    await stopCopilotSession(input.sessionId);
    return;
  }

  live.turns = live.turns
    .catch(() => undefined)
    .then(async () => {
      live.onEvent = input.onEvent;
      live.runtimeMode = input.runtimeMode;
      live.planning = input.intent === "plan";
      live.cancelled = false;
      live.muteUpdates = false;
      try {
        await applyModelSelection(live, input);
        if (live.cancelled) return;
        await applyRuntimeMode(live, input.runtimeMode, live.planning);
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
    if (liveByThread.get(input.sessionId) === live) {
      await stopCopilotSession(input.sessionId);
    }
    throw error;
  }
}

export async function steerCopilotTurn(input: SteerTurnInput): Promise<void> {
  const live = liveByThread.get(input.sessionId);
  if (!live?.activePrompt || live.cancelled)
    throw new Error("No active Copilot CLI turn");
  startPrompt(live, input);
}

export function respondCopilotApproval(
  sessionId: string,
  requestId: number,
  decision: ApprovalDecision,
): void {
  liveByThread.get(sessionId)?.approvals.get(requestId)?.(decision);
}

export async function cancelCopilotTurn(sessionId: string): Promise<void> {
  const live = liveByThread.get(sessionId);
  if (!live) {
    cancelledThreads.add(sessionId);
    return;
  }
  live.cancelled = true;
  live.muteUpdates = true;
  resolveApprovals(live);
  await live.acp
    .notify("session/cancel", { sessionId: live.acpSessionId })
    .catch(() => undefined);
  live.acp.rejectPending(new Error("cancelled"));
}

export async function stopCopilotSession(sessionId: string): Promise<void> {
  cancelledThreads.delete(sessionId);
  const live = liveByThread.get(sessionId);
  liveByThread.delete(sessionId);
  if (live) {
    live.muteUpdates = true;
    live.cancelled = true;
    resolveApprovals(live);
  }
  live?.acp.close();
  unwatchChild(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

export async function forgetCopilotSession(sessionId: string): Promise<void> {
  resumeByThread.delete(sessionId);
  await stopCopilotSession(sessionId);
}

export function bindCopilotSession(
  threadId: string,
  acpSessionId: string,
  cwd: string,
): void {
  const sessionId = acpSessionId.trim();
  if (!threadId || !sessionId || !cwd.trim()) return;
  resumeByThread.set(threadId, { acpSessionId: sessionId, cwd });
}

async function ensureLive(input: HarnessSessionInput): Promise<Live> {
  const existing = liveByThread.get(input.sessionId);
  if (existing && existing.cwd === input.cwd) return existing;
  if (existing) {
    resumeByThread.delete(input.sessionId);
    await stopCopilotSession(input.sessionId);
  }

  const resume = resumeByThread.get(input.sessionId);
  const canLoad = resume != null && resume.cwd === input.cwd;
  if (resume && resume.cwd !== input.cwd)
    resumeByThread.delete(input.sessionId);

  const { path } = await resolveCopilotBinary();
  const handlers: AcpHandlers = {};
  const acp = new AcpClient(input.sessionId, handlers);
  const liveRef: { current: Live | null } = { current: null };
  const muteGate = { current: false };

  handlers.onNotification = (method, params) => {
    if (muteGate.current) return;
    const live = liveRef.current;
    if (!live || live.muteUpdates) return;
    if (method !== "session/update") return;
    const events = eventsFromAcpUpdate(params);
    for (const event of live.subagents.route(params, events)) {
      live.onEvent(event);
    }
  };
  handlers.onRequest = (id, method, params) => {
    const live = liveRef.current;
    if (!live) {
      void acp.respondError(id, {
        code: -32601,
        message: `Method not found: ${method}`,
      }).catch(() => undefined);
      return;
    }
    void handleRequest(live, id, method, params);
  };

  const emit = (event: HarnessEvent) => {
    (liveRef.current?.onEvent ?? input.onEvent)(event);
  };
  watchChild(
    input.sessionId,
    (line) => acp.pushLine(line),
    (code) => {
      const live = liveRef.current;
      if (live) live.cancelled = true;
      acp.close(new Error("Copilot CLI exited"));
      liveByThread.delete(input.sessionId);
      emit({ type: "session.ended", code });
    },
    (line) => {
      console.debug("[monocode] copilot stderr", line);
      const message = copilotStderrAuthError(line);
      if (message) emit({ type: "session.error", message });
    },
  );

  try {
    await spawnChild(
      input.sessionId,
      path,
      ["--acp", "--stdio"],
      input.cwd,
      undefined,
      "copilot",
    );
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
      throw copilotStartupError(error);
    }

    let setup: unknown;
    let acpSessionId: string | undefined;
    let didLoad = false;
    if (canLoad && resume) {
      muteGate.current = true;
      try {
        setup = await acp.request(
          "session/load",
          { sessionId: resume.acpSessionId, cwd: input.cwd, mcpServers: [] },
          SESSION_TIMEOUT_MS,
        );
        acpSessionId = copilotSessionId(setup) ?? resume.acpSessionId;
        didLoad = true;
      } catch {
        setup = undefined;
        acpSessionId = undefined;
      } finally {
        muteGate.current = false;
      }
    }

    if (!acpSessionId) {
      try {
        setup = await acp.request(
          "session/new",
          { cwd: input.cwd, mcpServers: [] },
          SESSION_TIMEOUT_MS,
        );
      } catch (error) {
        throw copilotStartupError(error);
      }
      acpSessionId = copilotSessionId(setup);
    }
    if (!acpSessionId)
      throw new Error("Copilot CLI did not return a session id");

    const live: Live = {
      subagents: new AcpSubagents(),
      acp,
      acpSessionId,
      cwd: input.cwd,
      modelId: copilotCurrentModelId(setup) ?? "",
      modeId: "",
      muteUpdates: didLoad,
      cancelled: false,
      runtimeMode: input.runtimeMode,
      planning: input.intent === "plan",
      onEvent: input.onEvent,
      approvals: new Map(),
      turns: Promise.resolve(),
      activePrompt: null,
      wakePrompt: null,
    };
    liveRef.current = live;
    liveByThread.set(input.sessionId, live);
    resumeByThread.set(input.sessionId, { acpSessionId, cwd: input.cwd });
    live.onEvent({
      type: "session.providerBound",
      providerSessionId: acpSessionId,
    });
    live.onEvent({ type: "session.started" });
    return live;
  } catch (error) {
    acp.close(error instanceof Error ? error : new Error(String(error)));
    await stopCopilotSession(input.sessionId);
    throw error;
  }
}

async function applyModelSelection(
  live: Live,
  input: HarnessSessionInput,
): Promise<void> {
  const modelId = nativeModelId(input.model).trim();
  if (!modelId || modelId === "default" || modelId === live.modelId) return;
  await live.acp.request(
    "session/set_model",
    { sessionId: live.acpSessionId, modelId },
    CONTROL_TIMEOUT_MS,
  );
  live.modelId = modelId;
}

async function applyRuntimeMode(
  live: Live,
  runtimeMode: RuntimeMode,
  planning: boolean,
): Promise<void> {
  const modeId = copilotModeId(runtimeMode, planning);
  if (modeId === live.modeId) return;
  await live.acp.request(
    "session/set_mode",
    { sessionId: live.acpSessionId, modeId },
    CONTROL_TIMEOUT_MS,
  );
  live.modeId = modeId;
}

function startPrompt(
  live: Live,
  input: Pick<SteerTurnInput, "text" | "attachments">,
): boolean {
  const blocks = promptBlocks(input.text, input.attachments);
  if (blocks.length === 0) return false;
  const pending = live.acp.request<unknown>(
    "session/prompt",
    { sessionId: live.acpSessionId, prompt: blocks },
    PROMPT_TIMEOUT_MS,
  );
  live.activePrompt = pending;
  live.wakePrompt?.();
  // A replacement can fail before the superseded prompt settles.
  void pending.catch(() => undefined);
  return true;
}

async function prompt(live: Live, input: SendTurnInput): Promise<void> {
  try {
    if (!startPrompt(live, input)) return;
    while (live.activePrompt) {
      const pending = live.activePrompt;
      const replaced = new Promise<void>((resolve) => {
        live.wakePrompt = resolve;
      });
      let result: unknown;
      try {
        result = await Promise.race([pending, replaced]);
      } catch (error) {
        if (live.cancelled) return;
        if (live.activePrompt !== pending) continue;
        throw error;
      } finally {
        live.wakePrompt = null;
      }
      if (live.cancelled) return;
      if (live.activePrompt !== pending) continue;
      live.activePrompt = null;
      live.onEvent({ type: "message.completed" });
      live.onEvent({ type: "reasoning.completed" });
      const metrics = copilotMetricsFromPromptResult(result);
      if (metrics) live.onEvent(metrics);
    }
  } catch (error) {
    live.activePrompt = null;
    if (live.cancelled) return;
    const detail = error instanceof Error ? error.message : String(error);
    live.onEvent({
      type: "session.error",
      message: isHarnessAuthError(detail)
        ? `${detail.trim()}\n\n${COPILOT_AUTH_HELP}`
        : detail,
    });
    throw error;
  } finally {
    live.activePrompt = null;
    live.wakePrompt = null;
  }
}

async function handleRequest(
  live: Live,
  id: number,
  method: string,
  params: unknown,
): Promise<void> {
  if (method === "session/request_permission") {
    const request = permissionRequestFromAcp(params);
    if (live.cancelled || !live.activePrompt) {
      await respondPermission(
        live,
        id,
        permissionOptionId("deny", request.optionIds),
        request.optionIds,
      );
      return;
    }
    await handlePermission(live, id, request);
    return;
  }
  await live.acp
    .respondError(id, { code: -32601, message: `Method not found: ${method}` })
    .catch(() => undefined);
}

async function handlePermission(
  live: Live,
  id: number,
  request: GrokPermissionRequest,
): Promise<void> {
  if (request.callId) {
    live.onEvent({
      type: "tool.updated",
      callId: request.callId,
      title: request.title,
      kind: request.kind,
      preview: request.preview,
    });
  }

  if (live.planning) {
    const readOnly = request.kind === "read" || request.kind === "search";
    await respondPermission(
      live,
      id,
      permissionOptionId(readOnly ? "allow" : "deny", request.optionIds),
      request.optionIds,
    );
    return;
  }

  // The mode promises edits only, so an unclassified request still asks.
  const automatic =
    live.runtimeMode === "auto-accept-edits" && request.kind !== "edit"
      ? null
      : pickAutoOption(live.runtimeMode, request.kind, request.optionIds);
  if (automatic) {
    await respondPermission(live, id, automatic, request.optionIds);
    return;
  }

  live.onEvent({
    type: "approval.requested",
    requestId: id,
    title: request.title,
    kind: request.kind,
    callId: request.callId,
    preview: request.preview,
  });
  const decision = await new Promise<ApprovalDecision>((resolve) => {
    live.approvals.set(id, resolve);
  });
  live.approvals.delete(id);
  live.onEvent({ type: "approval.resolved", requestId: id, decision });
  await respondPermission(live, id, permissionOptionId(decision, request.optionIds), request.optionIds);
}

async function respondPermission(
  live: Live,
  id: number,
  optionId: string,
  optionIds: string[],
): Promise<void> {
  if (!optionIds.includes(optionId)) {
    await live.acp.respondError(id, { code: -32602, message: "No matching permission option advertised" });
    return;
  }
  await live.acp.respond(id, { outcome: { outcome: "selected", optionId } });
}

function resolveApprovals(live: Live): void {
  for (const resolve of live.approvals.values()) resolve("deny");
  live.approvals.clear();
}

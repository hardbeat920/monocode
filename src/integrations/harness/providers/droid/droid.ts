import {
  hasLiveCatalog,
  nativeModelId,
  setHarnessModels,
} from "../../../../features/sessions/model/models";
import type { RuntimeMode } from "../../../../features/sessions/model/session";
import { AcpClient, type AcpHandlers } from "../../core/acp";
import { AcpSubagents } from "../../core/acpSubagents";
import {
  killChild,
  resolveDroidBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import {
  DROID_ACP_ARGS,
  DROID_AUTH_HELP,
  droidConfigOptionsFrom,
  droidCurrentModelId,
  droidEffortConfig,
  droidEffortValue,
  droidErrorMessage,
  droidModeId,
  droidModelConfig,
  droidPromptBlocks,
  droidSessionId,
  droidSpecPlan,
  droidStartupError,
  isDroidAuthError,
  isDroidErrorEcho,
  modelsFromDroidSession,
  type DroidConfigOption,
} from "./droidProtocol";
import {
  eventsFromAcpUpdate,
  permissionOptionId,
  permissionRequestFromAcp,
  pickAutoOption,
} from "../grok/grokProtocol";
import type {
  ApprovalDecision,
  HarnessEvent,
  HarnessSessionInput,
  SendTurnInput,
} from "../../core/types";
import { refreshDroidCatalog } from "./droidCatalog";

type Live = {
  subagents: AcpSubagents;
  acp: AcpClient;
  acpSessionId: string;
  cwd: string;
  configOptions: DroidConfigOption[];
  modelId: string;
  modeId: string;
  muteUpdates: boolean;
  cancelled: boolean;
  runtimeMode: RuntimeMode;
  planning: boolean;
  onEvent: (event: HarnessEvent) => void;
  approvals: Map<number, (decision: ApprovalDecision) => void>;
  turns: Promise<void>;
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

/** Live Factory Droid adapter. Spawns `droid exec --output-format acp`. */
export async function sendDroidTurn(input: SendTurnInput): Promise<void> {
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
      await stopDroidSession(input.sessionId);
    }
    throw error;
  }
}

/** Droid runs one prompt at a time; MonoCode queues follow-ups (canSteer: false). */
export async function steerDroidTurn(): Promise<void> {
  throw new Error("Factory Droid cannot accept a message mid-turn");
}

export function respondDroidApproval(
  sessionId: string,
  requestId: number,
  decision: ApprovalDecision,
): void {
  liveByThread.get(sessionId)?.approvals.get(requestId)?.(decision);
}

export async function cancelDroidTurn(sessionId: string): Promise<void> {
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

export async function stopDroidSession(sessionId: string): Promise<void> {
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

export async function forgetDroidSession(sessionId: string): Promise<void> {
  resumeByThread.delete(sessionId);
  await stopDroidSession(sessionId);
}

export function bindDroidSession(
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
  if (existing && existing.cwd === input.cwd) {
    existing.onEvent = input.onEvent;
    existing.runtimeMode = input.runtimeMode;
    existing.planning = input.intent === "plan";
    return existing;
  }
  if (existing) {
    resumeByThread.delete(input.sessionId);
    await stopDroidSession(input.sessionId);
  }

  const resume = resumeByThread.get(input.sessionId);
  const canLoad = resume != null && resume.cwd === input.cwd;
  if (resume && resume.cwd !== input.cwd)
    resumeByThread.delete(input.sessionId);

  const { path } = await resolveDroidBinary();
  const handlers: AcpHandlers = {};
  const acp = new AcpClient(input.sessionId, handlers);
  const liveRef: { current: Live | null } = { current: null };
  const muteGate = { current: false };

  handlers.onNotification = (method, params) => {
    const live = liveRef.current;
    // Config snapshots matter even while a session/load replay is muted.
    if (live && method === "session/update") {
      const options = droidConfigOptionsFrom(params);
      if (options) {
        live.configOptions = options;
        return;
      }
    }
    if (muteGate.current) return;
    if (!live || live.muteUpdates) return;
    handleNotification(live, method, params);
  };
  handlers.onRequest = (id, method, params) => {
    const live = liveRef.current;
    if (!live) {
      void acp
        .respondError(id, {
          code: -32601,
          message: `Method not found: ${method}`,
        })
        .catch(() => undefined);
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
      acp.close(new Error("Factory Droid exited"));
      liveByThread.delete(input.sessionId);
      emit({ type: "session.ended", code });
    },
    (line) => {
      console.debug("[monocode] droid stderr", line);
      if (isDroidAuthError(line)) {
        emit({
          type: "session.error",
          message: `${line.trim()}\n\n${DROID_AUTH_HELP}`,
        });
      }
    },
  );

  await spawnChild(input.sessionId, path, DROID_ACP_ARGS, input.cwd);

  try {
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
      throw droidStartupError(error);
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
        acpSessionId = droidSessionId(setup) ?? resume.acpSessionId;
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
        throw droidStartupError(error);
      }
      acpSessionId = droidSessionId(setup);
    }
    if (!acpSessionId)
      throw new Error("Factory Droid did not return a session id");

    const configOptions = droidConfigOptionsFrom(setup) ?? [];
    const live: Live = {
      subagents: new AcpSubagents(),
      acp,
      acpSessionId,
      cwd: input.cwd,
      configOptions,
      modelId: droidCurrentModelId(setup) ?? "",
      modeId: "",
      muteUpdates: didLoad,
      cancelled: false,
      runtimeMode: input.runtimeMode,
      planning: input.intent === "plan",
      onEvent: input.onEvent,
      approvals: new Map(),
      turns: Promise.resolve(),
    };
    liveRef.current = live;
    liveByThread.set(input.sessionId, live);
    resumeByThread.set(input.sessionId, { acpSessionId, cwd: input.cwd });
    live.onEvent({
      type: "session.providerBound",
      providerSessionId: acpSessionId,
    });
    live.onEvent({ type: "session.started" });
    if (!hasLiveCatalog("droid")) {
      // A running session already knows Droid's models; show them now and
      // let the catalog probe fill in per-model reasoning levels.
      const models = modelsFromDroidSession(setup);
      if (models.length > 0) setHarnessModels("droid", models);
      void refreshDroidCatalog();
    }
    return live;
  } catch (error) {
    acp.close(error instanceof Error ? error : new Error(String(error)));
    await stopDroidSession(input.sessionId);
    throw error;
  }
}

async function setConfigOption(
  live: Live,
  configId: string,
  value: string,
): Promise<void> {
  const current = live.configOptions.find((option) => option.id === configId);
  if (current?.currentValue === value) return;
  const result = await live.acp.request<unknown>(
    "session/set_config_option",
    { sessionId: live.acpSessionId, configId, value },
    CONTROL_TIMEOUT_MS,
  );
  const options = droidConfigOptionsFrom(result);
  if (options) {
    live.configOptions = options;
  } else if (current) {
    current.currentValue = value;
  }
}

async function applyModelSelection(
  live: Live,
  input: HarnessSessionInput,
): Promise<void> {
  const modelId = nativeModelId(input.model).trim();
  if (modelId && modelId !== "default" && modelId !== live.modelId) {
    const configId = droidModelConfig(live.configOptions)?.id ?? "model";
    try {
      await setConfigOption(live, configId, modelId);
    } catch {
      await live.acp.request(
        "session/set_model",
        { sessionId: live.acpSessionId, modelId },
        CONTROL_TIMEOUT_MS,
      );
    }
    live.modelId = modelId;
  }

  // Reasoning levels are per model, so apply effort after the model switch.
  const effort = droidEffortConfig(live.configOptions);
  const value = droidEffortValue(effort, input.modelSettings);
  if (effort && value) {
    await setConfigOption(live, effort.id, value).catch((error: unknown) => {
      console.debug("[monocode] droid effort", error);
    });
  }
}

async function applyRuntimeMode(
  live: Live,
  runtimeMode: RuntimeMode,
  planning: boolean,
): Promise<void> {
  const modeId = droidModeId(runtimeMode, planning);
  if (modeId === live.modeId) return;
  try {
    await live.acp.request(
      "session/set_mode",
      { sessionId: live.acpSessionId, modeId },
      CONTROL_TIMEOUT_MS,
    );
  } catch {
    await setConfigOption(live, "autonomy_level", modeId);
  }
  live.modeId = modeId;
}

async function prompt(live: Live, input: SendTurnInput): Promise<void> {
  try {
    const blocks = droidPromptBlocks(input.text, input.attachments);
    if (blocks.length === 0) return;
    await live.acp.request(
      "session/prompt",
      { sessionId: live.acpSessionId, prompt: blocks },
      PROMPT_TIMEOUT_MS,
    );
  } catch (error) {
    if (live.cancelled) return;
    const detail = droidErrorMessage(error);
    live.onEvent({
      type: "session.error",
      message: isDroidAuthError(detail)
        ? `${detail}\n\n${DROID_AUTH_HELP}`
        : detail,
    });
    throw error;
  }
}

function handleNotification(live: Live, method: string, params: unknown): void {
  if (method !== "session/update") return;
  if (isDroidErrorEcho(params)) return;
  const events = eventsFromAcpUpdate(params);
  for (const event of live.subagents.route(params, events)) {
    live.onEvent(event);
  }
}

async function handleRequest(
  live: Live,
  id: number,
  method: string,
  params: unknown,
): Promise<void> {
  if (method === "session/request_permission") {
    await handlePermission(live, id, params);
    return;
  }
  await live.acp
    .respondError(id, { code: -32601, message: `Method not found: ${method}` })
    .catch(() => undefined);
}

async function handlePermission(
  live: Live,
  id: number,
  params: unknown,
): Promise<void> {
  const request = permissionRequestFromAcp(params);
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
    // Spec mode ends by asking to leave it. Surface the spec as MonoCode's
    // plan and stay read-only; the user decides whether to build it.
    const plan = droidSpecPlan(params);
    if (plan) live.onEvent({ type: "plan", text: plan, streaming: false });
    const readOnly = request.kind === "read" || request.kind === "search";
    await respondPermission(
      live,
      id,
      permissionOptionId(readOnly ? "allow" : "deny", request.optionIds),
    );
    return;
  }

  const automatic = pickAutoOption(
    live.runtimeMode,
    request.kind,
    request.optionIds,
  );
  if (automatic) {
    await respondPermission(live, id, automatic);
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
  await respondPermission(
    live,
    id,
    permissionOptionId(decision, request.optionIds),
  );
}

async function respondPermission(
  live: Live,
  id: number,
  optionId: string,
): Promise<void> {
  await live.acp.respond(id, {
    outcome: { outcome: "selected", optionId },
  });
}

function resolveApprovals(live: Live): void {
  for (const resolve of live.approvals.values()) resolve("deny");
  live.approvals.clear();
}

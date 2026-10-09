import {
  hasLiveCatalog,
  nativeModelId,
  setHarnessModels,
} from "../../../../features/sessions/model/models";
import type { RuntimeMode } from "../../../../features/sessions/model/session";
import { AcpClient, type AcpHandlers } from "../../core/acp";
import { AcpSubagents } from "../../core/acpSubagents";
import {
  hasHeadlessChildBackend,
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
  configRevision: number;
  modelId: string;
  modeId: string;
  modeRevision: number;
  muteUpdates: boolean;
  cancelled: boolean;
  runtimeMode: RuntimeMode;
  planning: boolean;
  onEvent: (event: HarnessEvent) => void;
  approvals: Map<number, (decision: ApprovalDecision | null) => void>;
  toolKinds: Map<string, string>;
  permissionTasks: Set<Promise<void>>;
  closed: boolean;
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
const startingByThread = new Map<string, AcpClient>();
const resumeByThread = new Map<string, Resume>();
const generations = new Map<string, object>();
const queues = new Map<string, Promise<void>>();

function checkGeneration(sessionId: string, generation: object): void {
  if (generations.get(sessionId) !== generation) throw new Error("cancelled");
}

/** Live Factory Droid adapter. Spawns `droid exec --output-format acp`. */
export async function sendDroidTurn(input: SendTurnInput): Promise<void> {
  const generation = generations.get(input.sessionId) ?? {};
  generations.set(input.sessionId, generation);
  const previous = queues.get(input.sessionId) ?? Promise.resolve();
  const turn = previous
    .catch(() => undefined)
    .then(async () => {
      if (generations.get(input.sessionId) !== generation) return;
      let live: Live | undefined;
      try {
        live = await ensureLive(input, generation);
        checkGeneration(input.sessionId, generation);
        live.onEvent = input.onEvent;
        live.runtimeMode = input.runtimeMode;
        live.planning = input.intent === "plan";
        live.cancelled = false;
        live.muteUpdates = false;
        live.toolKinds.clear();
        await applyModelSelection(live, input);
        checkGeneration(input.sessionId, generation);
        await applyRuntimeMode(live, input.runtimeMode, live.planning);
        checkGeneration(input.sessionId, generation);
        await prompt(live, input);
      } catch (error) {
        if (generations.get(input.sessionId) !== generation || live?.cancelled)
          return;
        if (liveByThread.get(input.sessionId) === live || !live) {
          await stopDroidSession(input.sessionId);
        }
        throw error;
      }
    });
  queues.set(input.sessionId, turn);
  try {
    await turn;
  } finally {
    if (queues.get(input.sessionId) === turn) queues.delete(input.sessionId);
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
  await stopDroidSession(sessionId, true, true);
}

export async function stopDroidSession(
  sessionId: string,
  invalidate = true,
  sendCancel = false,
): Promise<void> {
  if (invalidate) generations.delete(sessionId);
  const previous = queues.get(sessionId) ?? Promise.resolve();
  const cleanup = stopConnection(sessionId, sendCancel);
  const barrier = Promise.allSettled([previous, cleanup]).then(() => undefined);
  queues.set(sessionId, barrier);
  try {
    await cleanup;
  } finally {
    void barrier.then(() => {
      if (queues.get(sessionId) === barrier) queues.delete(sessionId);
    });
  }
}

async function stopConnection(
  sessionId: string,
  sendCancel: boolean,
): Promise<void> {
  const live = liveByThread.get(sessionId);
  liveByThread.delete(sessionId);
  const starting = startingByThread.get(sessionId);
  startingByThread.delete(sessionId);
  starting?.close();
  if (live) {
    live.muteUpdates = true;
    live.cancelled = true;
    resolveApprovals(live);
    if (sendCancel)
      await live.acp
        .notify("session/cancel", { sessionId: live.acpSessionId })
        .catch(() => undefined);
    await Promise.allSettled([...live.permissionTasks]);
    live.closed = true;
    live.acp.close();
  }
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

async function ensureLive(
  input: HarnessSessionInput,
  generation: object,
): Promise<Live> {
  const existing = liveByThread.get(input.sessionId);
  if (existing && existing.cwd === input.cwd) {
    return existing;
  }
  if (existing) {
    resumeByThread.delete(input.sessionId);
    await stopDroidSession(input.sessionId, false);
    checkGeneration(input.sessionId, generation);
  }

  const resume = resumeByThread.get(input.sessionId);
  const canLoad = resume != null && resume.cwd === input.cwd;
  if (resume && resume.cwd !== input.cwd)
    resumeByThread.delete(input.sessionId);

  const { path } = await resolveDroidBinary();
  checkGeneration(input.sessionId, generation);
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
        syncConfig(live, options);
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
    let complete!: () => void;
    const task = new Promise<void>((resolve) => {
      complete = resolve;
    });
    live.permissionTasks.add(task);
    void (async () => {
      try {
        await handleRequest(live, id, method, params);
      } catch (error) {
        if (!live.closed && !live.cancelled) {
          live.onEvent({
            type: "session.error",
            message: droidErrorMessage(error),
          });
        }
      } finally {
        live.permissionTasks.delete(task);
        complete();
      }
    })().catch(() => undefined);
  };

  const emit = (event: HarnessEvent) => {
    (liveRef.current?.onEvent ?? input.onEvent)(event);
  };
  watchChild(
    input.sessionId,
    (line) => acp.pushLine(line),
    (code) => {
      const live = liveRef.current;
      if (live) {
        live.closed = true;
        live.muteUpdates = true;
        resolveApprovals(live);
      }
      acp.close(new Error("Factory Droid exited"));
      if (liveByThread.get(input.sessionId) === live)
        liveByThread.delete(input.sessionId);
      emit({ type: "session.ended", code });
      unwatchChild(input.sessionId);
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

  startingByThread.set(input.sessionId, acp);
  try {
    await spawnChild(
      input.sessionId,
      path,
      DROID_ACP_ARGS,
      input.cwd,
      undefined,
      "droid",
    );

    checkGeneration(input.sessionId, generation);
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

    checkGeneration(input.sessionId, generation);
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
      } finally {
        muteGate.current = false;
      }
    }

    checkGeneration(input.sessionId, generation);
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
    checkGeneration(input.sessionId, generation);
    if (!acpSessionId)
      throw new Error("Factory Droid did not return a session id");

    const configOptions = droidConfigOptionsFrom(setup) ?? [];
    const live: Live = {
      subagents: new AcpSubagents(),
      acp,
      acpSessionId,
      cwd: input.cwd,
      configOptions,
      configRevision: 0,
      modelId: droidCurrentModelId(setup) ?? "",
      modeId: "",
      modeRevision: 0,
      muteUpdates: didLoad,
      cancelled: false,
      runtimeMode: input.runtimeMode,
      planning: input.intent === "plan",
      onEvent: input.onEvent,
      approvals: new Map(),
      toolKinds: new Map(),
      permissionTasks: new Set(),
      closed: false,
    };
    startingByThread.delete(input.sessionId);
    liveRef.current = live;
    liveByThread.set(input.sessionId, live);
    resumeByThread.set(input.sessionId, { acpSessionId, cwd: input.cwd });
    live.onEvent({
      type: "session.providerBound",
      providerSessionId: acpSessionId,
    });
    checkGeneration(input.sessionId, generation);
    live.onEvent({ type: "session.started" });
    checkGeneration(input.sessionId, generation);
    if (!hasLiveCatalog("droid")) {
      // A running session already knows Droid's models; show them now and
      // let the catalog probe fill in per-model reasoning levels.
      const models = modelsFromDroidSession(setup);
      if (models.length > 0) setHarnessModels("droid", models, false);
      // Remote hosts discover catalogs with their project directory.
      if (!hasHeadlessChildBackend()) void refreshDroidCatalog();
    }
    return live;
  } catch (error) {
    acp.close(error instanceof Error ? error : new Error(String(error)));
    await stopDroidSession(input.sessionId, false);
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
  const revision = live.configRevision;
  const result = await live.acp.request<unknown>(
    "session/set_config_option",
    { sessionId: live.acpSessionId, configId, value },
    CONTROL_TIMEOUT_MS,
  );
  const options = droidConfigOptionsFrom(result);
  if (options) {
    syncConfig(live, options);
    const effective = options.find(
      (option) => option.id === configId,
    )?.currentValue;
    if (
      configId !== droidModelConfig(options)?.id &&
      effective != null &&
      effective !== value
    ) {
      throw new Error(`Factory Droid did not apply the requested ${configId}`);
    }
  } else if (
    current &&
    live.configRevision === revision &&
    configId !== droidModelConfig(live.configOptions)?.id
  ) {
    current.currentValue = value;
  }
}

async function applyModelSelection(
  live: Live,
  input: HarnessSessionInput,
): Promise<void> {
  const modelId = nativeModelId(input.model).trim();
  const requestedEffort =
    input.modelSettings?.effort ?? input.modelSettings?.reasoning;
  const switching =
    modelId && modelId !== "default" && modelId !== live.modelId;
  if (switching) {
    const revision = live.configRevision;
    const configId = droidModelConfig(live.configOptions)?.id ?? "model";
    try {
      await setConfigOption(live, configId, modelId);
    } catch (error) {
      if (
        !(error instanceof Error) ||
        !/method not found|unsupported/i.test(error.message)
      )
        throw error;
      await live.acp.request(
        "session/set_model",
        { sessionId: live.acpSessionId, modelId },
        CONTROL_TIMEOUT_MS,
      );
    }
    if (live.configRevision === revision) live.modelId = modelId;
  }

  // Reasoning levels are per model, so apply effort after the model switch.
  if (switching && requestedEffort) {
    const deadline = Date.now() + CONTROL_TIMEOUT_MS;
    while (droidModelConfig(live.configOptions)?.currentValue !== modelId) {
      if (live.cancelled || live.closed) return;
      if (Date.now() >= deadline)
        throw new Error(
          "Factory Droid did not confirm the selected model configuration",
        );
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
  }
  const effort = droidEffortConfig(live.configOptions);
  const value = droidEffortValue(effort, input.modelSettings);
  if (requestedEffort && !value) {
    throw new Error(
      `Factory Droid does not support reasoning effort ${requestedEffort} for the selected model`,
    );
  }
  if (effort && value) await setConfigOption(live, effort.id, value);
}

async function applyRuntimeMode(
  live: Live,
  runtimeMode: RuntimeMode,
  planning: boolean,
): Promise<void> {
  const modeId = droidModeId(runtimeMode, planning);
  if (modeId === live.modeId) return;
  const revision = live.modeRevision;
  try {
    await live.acp.request(
      "session/set_mode",
      { sessionId: live.acpSessionId, modeId },
      CONTROL_TIMEOUT_MS,
    );
  } catch {
    await setConfigOption(live, "autonomy_level", modeId);
  }
  if (live.modeRevision === revision) live.modeId = modeId;
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
  const update = (
    params as { update?: { sessionUpdate?: string; currentModeId?: string } }
  )?.update;
  if (
    update?.sessionUpdate === "current_mode_update" &&
    typeof update.currentModeId === "string"
  ) {
    live.modeId = update.currentModeId;
    live.modeRevision += 1;
  }
  if (isDroidErrorEcho(params)) return;
  const events = eventsFromAcpUpdate(params);
  for (const event of live.subagents.route(params, events)) {
    if (
      (event.type === "tool.started" || event.type === "tool.updated") &&
      event.kind
    ) {
      live.toolKinds.set(event.callId, event.kind);
    }
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
  if (live.closed) return;
  if (live.cancelled) {
    await respondPermission(live, id, null);
    return;
  }
  const request = permissionRequestFromAcp(params);
  request.kind ??= request.callId
    ? live.toolKinds.get(request.callId)
    : undefined;
  if (request.callId) {
    live.onEvent({
      type: "tool.updated",
      callId: request.callId,
      title: request.title,
      kind: request.kind,
      preview: request.preview,
    });
  }
  if (live.cancelled || live.closed) {
    await respondPermission(live, id, null);
    return;
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
      permissionOptionId(
        readOnly ? "allow" : "deny",
        request.optionIds,
        request.optionKinds,
      ),
    );
    return;
  }

  const automatic = pickAutoOption(
    live.runtimeMode,
    request.kind,
    request.optionIds,
    request.optionKinds,
  );
  if (automatic) {
    await respondPermission(live, id, automatic);
    return;
  }

  const pending = new Promise<ApprovalDecision | null>((resolve) => {
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
  live.onEvent({
    type: "approval.resolved",
    requestId: id,
    decision: decision ?? "deny",
  });
  await respondPermission(
    live,
    id,
    decision && !live.cancelled
      ? permissionOptionId(decision, request.optionIds, request.optionKinds)
      : null,
  );
}

async function respondPermission(
  live: Live,
  id: number,
  optionId: string | null,
): Promise<void> {
  if (live.closed) return;
  if (live.cancelled) optionId = null;
  await live.acp.respond(id, {
    outcome: optionId
      ? { outcome: "selected", optionId }
      : { outcome: "cancelled" },
  });
}

function resolveApprovals(live: Live): void {
  for (const resolve of live.approvals.values()) resolve(null);
  live.approvals.clear();
}

function syncConfig(live: Live, options: DroidConfigOption[]): void {
  live.configOptions = options;
  live.configRevision += 1;
  live.modelId = droidModelConfig(options)?.currentValue ?? live.modelId;
  const mode = options.find(
    (option) => option.category === "mode" || option.id === "autonomy_level",
  )?.currentValue;
  if (mode != null) {
    live.modeId = mode;
    live.modeRevision += 1;
  }
}

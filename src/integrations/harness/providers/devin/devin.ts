import { nativeModelId } from "../../../../features/sessions/model/models";
import type { RuntimeMode } from "../../../../features/sessions/model/session";
import { AcpSubagents } from "../../core/acpSubagents";
import { DevinSubagents } from "./devinSubagents";
import {
  killChild,
  resolveDevinBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import { JsonRpcClient, type JsonRpcId } from "../../core/jsonRpc";
import type { NativeCommand } from "../../core/nativeCommands";
import type {
  ApprovalDecision,
  CompactContextInput,
  HarnessEvent,
  HarnessSessionInput,
  SendTurnInput,
  SteerTurnInput,
} from "../../core/types";
import {
  asRecord,
  permissionOptionId,
  permissionRequestFromAcp,
  readConfigOptions,
  resolveSettingConfigId,
  sessionIdFromResult,
  type SessionConfigOption,
} from "../antigravity/antigravityProtocol";
import { readDevinApiKey } from "./devinAuth";
import {
  DEVIN_AUTH_HELP,
  devinAsksBeforeEdits,
  devinAuthenticateParams,
  devinAuthMethodId,
  devinCommandsFromUpdate,
  devinEventsFromUpdate,
  devinModelChoices,
  devinModelFamilies,
  devinModelValue,
  devinModeId,
  devinNearestEffort,
  devinPermissionCommand,
  devinPromptBlocks,
  devinRestoreError,
  devinSessionMissing,
  devinSessionTitle,
  devinStartupError,
  devinToolInfo,
  devinTurnError,
  type DevinModeId,
  type DevinModelFamily,
  type DevinToolInfo,
} from "./devinProtocol";

type Live = {
  threadId: string;
  subagents: AcpSubagents;
  devinSubagents: DevinSubagents;
  rpc: JsonRpcClient;
  acpSessionId: string;
  cwd: string;
  /** Spawned with the Supervised edit-approval rule. */
  asksBeforeEdits: boolean;
  /** A saved model Devin no longer lists, reported once. */
  staleModel?: string;
  configOptions: SessionConfigOption[];
  /** Effort/speed variants behind each grouped picker model. */
  modelFamilies: DevinModelFamily[];
  modeId: DevinModeId | "";
  muteUpdates: boolean;
  cancelled: boolean;
  /** Permission requests are only answered while a prompt is running. */
  promptInFlight: boolean;
  runtimeMode: RuntimeMode;
  planning: boolean;
  onEvent: (event: HarnessEvent) => void;
  approvals: Map<number, (decision: ApprovalDecision) => void>;
  tools: Map<string, DevinToolInfo>;
  turns: Promise<void>;
};

type Resume = { acpSessionId: string; cwd: string };

/** A child that is still starting; stop and cancel must reach it before it is live. */
type Startup = {
  rpc: JsonRpcClient | null;
  /** Stopped or deleted: tear the child down and never bind it. */
  stopped: boolean;
  /** The turn was cancelled: keep the child, skip the prompt. */
  cancelled: boolean;
};

const INIT_TIMEOUT_MS = 20_000;
const AUTH_TIMEOUT_MS = 5 * 60_000;
const SESSION_TIMEOUT_MS = 45_000;
const CONTROL_TIMEOUT_MS = 20_000;
const PROMPT_TIMEOUT_MS = 30 * 60_000;
const TITLE_TIMEOUT_MS = 90_000;
const TITLE_LIMIT = 50;

export const DEVIN_CLIENT_CAPABILITIES = {
  fs: { readTextFile: false, writeTextFile: false },
  terminal: false,
  session: { configOptions: { boolean: {} } },
  // Streams each subagent's own thoughts and replies, tagged with its agent
  // id, so they land on its row instead of only its tool calls.
  _meta: { "cognition.ai/subagentSupport": true },
};

const liveByThread = new Map<string, Live>();
const resumeByThread = new Map<string, Resume>();
const startups = new Map<string, Startup>();
const commandsByThread = new Map<string, NativeCommand[]>();
const commandListeners = new Set<(threadId: string, commands: NativeCommand[]) => void>();
const titleWaiters = new Map<string, Set<(title: string) => void>>();
let approvalSeq = 0;

/** Live Devin adapter. Spawns `devin acp` over standard ACP. */
export async function sendDevinTurn(input: SendTurnInput): Promise<void> {
  await runTurn(input, async (live) => {
    await applyModelSelection(live, input);
    if (live.cancelled) return;
    await applyRuntimeMode(live, input.runtimeMode, live.planning);
    if (live.cancelled) return;
    await prompt(live, devinPromptBlocks(input.text, input.attachments));
  });
}

/** Devin advertises `/compact`; run it as an ordinary command turn. */
export async function compactDevinContext(
  input: CompactContextInput,
): Promise<void> {
  await runTurn(input, (live) => prompt(live, devinPromptBlocks("/compact")));
}

async function runTurn(
  input: HarnessSessionInput,
  body: (live: Live) => Promise<void>,
): Promise<void> {
  const live = await ensureLive(input);
  if (!live) return;

  live.onEvent = input.onEvent;
  live.runtimeMode = input.runtimeMode;
  live.planning = input.intent === "plan";
  live.turns = live.turns
    .catch(() => undefined)
    .then(async () => {
      live.cancelled = false;
      live.muteUpdates = false;
      try {
        await body(live);
      } catch (error) {
        if (live.cancelled) return;
        throw error;
      }
    });

  try {
    await live.turns;
  } catch (error) {
    if (liveByThread.get(input.sessionId) === live) {
      await stopDevinSession(input.sessionId);
    }
    throw error;
  }
}

export async function steerDevinTurn(_input: SteerTurnInput): Promise<void> {
  throw new Error("Devin does not support steering an in-flight turn");
}

export function respondDevinApproval(
  sessionId: string,
  requestId: number,
  decision: ApprovalDecision,
): void {
  liveByThread.get(sessionId)?.approvals.get(requestId)?.(decision);
}

export async function cancelDevinTurn(sessionId: string): Promise<void> {
  const live = liveByThread.get(sessionId);
  if (!live) {
    // An idle thread has nothing to cancel; a stale marker would drop the next prompt.
    const startup = startups.get(sessionId);
    if (startup) startup.cancelled = true;
    return;
  }
  live.cancelled = true;
  live.muteUpdates = true;
  resolveApprovals(live);
  live.rpc.rejectPending(new Error("cancelled"));
  await live.rpc
    .notify("session/cancel", { sessionId: live.acpSessionId })
    .catch(() => undefined);
}

export async function stopDevinSession(sessionId: string): Promise<void> {
  const startup = startups.get(sessionId);
  if (startup) {
    startups.delete(sessionId);
    startup.stopped = true;
    // Fails the in-flight initialize/authenticate/session request at once.
    startup.rpc?.close(new Error("Devin session stopped"));
  }
  const live = liveByThread.get(sessionId);
  liveByThread.delete(sessionId);
  if (live) {
    live.muteUpdates = true;
    live.cancelled = true;
    resolveApprovals(live);
    live.rpc.close();
  }
  unwatchChild(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

export async function forgetDevinSession(sessionId: string): Promise<void> {
  resumeByThread.delete(sessionId);
  commandsByThread.delete(sessionId);
  await stopDevinSession(sessionId);
}

export function bindDevinSession(
  threadId: string,
  acpSessionId: string,
  cwd: string,
): void {
  const sessionId = acpSessionId.trim();
  if (!threadId || !sessionId || !cwd.trim()) return;
  resumeByThread.set(threadId, { acpSessionId: sessionId, cwd });
}

/** The slash commands the live Devin session advertised, if any yet. */
export function devinSessionCommands(threadId: string): NativeCommand[] | undefined {
  return commandsByThread.get(threadId);
}

export function subscribeDevinCommands(
  listener: (threadId: string, commands: NativeCommand[]) => void,
): () => void {
  commandListeners.add(listener);
  return () => {
    commandListeners.delete(listener);
  };
}

/**
 * Devin titles its sessions on its own and reports the result in
 * `session_info_update`; wait for that instead of paying for a second model.
 */
export function waitForDevinSessionTitle(
  threadId: string,
  timeoutMs = TITLE_TIMEOUT_MS,
): Promise<string | null> {
  return new Promise((resolve) => {
    let waiters = titleWaiters.get(threadId);
    if (!waiters) titleWaiters.set(threadId, (waiters = new Set()));
    const done = (title: string | null) => {
      clearTimeout(timer);
      waiters.delete(onTitle);
      if (waiters.size === 0 && titleWaiters.get(threadId) === waiters)
        titleWaiters.delete(threadId);
      resolve(title);
    };
    const onTitle = (title: string) => done(title.slice(0, TITLE_LIMIT).trim());
    const timer = setTimeout(() => done(null), timeoutMs);
    waiters.add(onTitle);
  });
}

/**
 * Spawn and authenticate a `devin acp` child. The CLI's stored key is used
 * when present; otherwise Devin's own browser sign-in runs for this process,
 * unless the caller is a background probe that must never open a browser.
 */
export async function startDevinAcp(
  childId: string,
  cwd: string,
  rpc: JsonRpcClient,
  options: { allowBrowser?: boolean; askBeforeEdits?: boolean } = {},
): Promise<void> {
  const { path } = await resolveDevinBinary();
  const apiKey = await readDevinApiKey(path);
  if (!apiKey && options.allowBrowser === false) {
    throw new Error(`Devin is not logged in. ${DEVIN_AUTH_HELP}`);
  }
  // Stopped while the binary and login were being resolved: spawn nothing.
  if (rpc.isClosed) throw new Error("Devin session stopped");
  await spawnChild(
    childId,
    path,
    ["acp"],
    cwd,
    undefined,
    "devin",
    undefined,
    options.askBeforeEdits,
  );
  let init: unknown;
  try {
    init = await rpc.request(
      "initialize",
      {
        protocolVersion: 1,
        clientCapabilities: DEVIN_CLIENT_CAPABILITIES,
        clientInfo: { name: "monocode", version: "0.1.0" },
      },
      INIT_TIMEOUT_MS,
    );
  } catch (error) {
    throw devinStartupError(error);
  }
  try {
    await rpc.request(
      "authenticate",
      devinAuthenticateParams(devinAuthMethodId(init), apiKey),
      apiKey ? INIT_TIMEOUT_MS : AUTH_TIMEOUT_MS,
    );
  } catch (error) {
    throw devinStartupError(error);
  }
}

/**
 * The thread's live child, started when needed. Null when the session was
 * stopped, deleted or cancelled before the child finished starting.
 */
async function ensureLive(input: HarnessSessionInput): Promise<Live | null> {
  const existing = liveByThread.get(input.sessionId);
  const sameCwd = existing?.cwd === input.cwd;
  if (
    existing &&
    sameCwd &&
    existing.asksBeforeEdits === devinAsksBeforeEdits(input.runtimeMode)
  ) {
    existing.onEvent = input.onEvent;
    existing.runtimeMode = input.runtimeMode;
    existing.planning = input.intent === "plan";
    return existing;
  }
  if (existing) {
    // Toggling Supervised restarts the child; session/load resumes the same
    // conversation. Another directory is another conversation.
    if (!sameCwd) resumeByThread.delete(input.sessionId);
    await stopDevinSession(input.sessionId);
  }

  const startup: Startup = { rpc: null, stopped: false, cancelled: false };
  startups.set(input.sessionId, startup);
  try {
    const live = await startLive(input, startup);
    return startup.cancelled ? null : live;
  } catch (error) {
    if (startup.stopped) return null;
    throw error;
  } finally {
    if (startups.get(input.sessionId) === startup) startups.delete(input.sessionId);
  }
}

async function startLive(input: HarnessSessionInput, startup: Startup): Promise<Live> {
  const resume = resumeByThread.get(input.sessionId);
  const canLoad = resume != null && resume.cwd === input.cwd;
  if (resume && !canLoad) resumeByThread.delete(input.sessionId);

  const liveRef: { current: Live | null } = { current: null };
  const muteGate = { current: false };
  const rpc = new JsonRpcClient(input.sessionId, {
    onNotification: (method, params) => {
      if (method !== "session/update") return;
      handleUpdate(input.sessionId, liveRef.current, params, muteGate.current);
    },
    onRequest: (id, method, params) => {
      const live = liveRef.current;
      if (live && method === "session/request_permission") {
        void handlePermission(live, id, params).catch(() => undefined);
        return;
      }
      void rpc
        .respondError(id, { code: -32601, message: `Method not found: ${method}` })
        .catch(() => undefined);
    },
  }, { includeJsonrpc: true, label: "devin" });
  startup.rpc = rpc;

  const emit = (event: HarnessEvent) => {
    (liveRef.current?.onEvent ?? input.onEvent)(event);
  };
  watchChild(
    input.sessionId,
    (line) => rpc.pushLine(line),
    (code) => {
      const live = liveRef.current;
      if (live) {
        live.cancelled = true;
        resolveApprovals(live);
      }
      rpc.close(new Error("Devin exited"));
      if (liveByThread.get(input.sessionId) === live)
        liveByThread.delete(input.sessionId);
      emit({ type: "session.ended", code });
    },
    (line) => {
      console.debug("[monocode] devin stderr", line);
    },
  );

  try {
    const asksBeforeEdits = devinAsksBeforeEdits(input.runtimeMode);
    await startDevinAcp(input.sessionId, input.cwd, rpc, {
      askBeforeEdits: asksBeforeEdits,
    });

    let setup: unknown;
    let acpSessionId: string | undefined;
    let didLoad = false;
    if (canLoad && resume) {
      // session/load replays the whole conversation as updates; the
      // transcript already holds it.
      muteGate.current = true;
      try {
        setup = await rpc.request(
          "session/load",
          { sessionId: resume.acpSessionId, cwd: input.cwd, mcpServers: [] },
          SESSION_TIMEOUT_MS,
        );
        acpSessionId = sessionIdFromResult(setup) ?? resume.acpSessionId;
        didLoad = true;
      } catch (error) {
        // Only a conversation Devin no longer has may be replaced. A timeout
        // or other failure keeps the binding so the next turn retries it.
        if (startup.stopped || !devinSessionMissing(error)) {
          throw devinRestoreError(error);
        }
        setup = undefined;
      } finally {
        muteGate.current = false;
      }
    }

    if (!acpSessionId) {
      try {
        setup = await rpc.request(
          "session/new",
          { cwd: input.cwd, mcpServers: [] },
          SESSION_TIMEOUT_MS,
        );
      } catch (error) {
        throw devinStartupError(error);
      }
      acpSessionId = sessionIdFromResult(setup);
      if (acpSessionId && canLoad) {
        emit({
          type: "status",
          text: "Devin could not restore the previous conversation — starting a new session.",
        });
      }
    }
    if (!acpSessionId) throw new Error("Devin did not return a session id");
    // Stopped or deleted while starting: a late bind would resurrect it.
    if (startup.stopped) throw new Error("Devin session stopped");

    const live: Live = {
      threadId: input.sessionId,
      subagents: new AcpSubagents(),
      devinSubagents: new DevinSubagents(),
      rpc,
      acpSessionId,
      cwd: input.cwd,
      asksBeforeEdits,
      configOptions: readConfigOptions(asRecord(setup)?.configOptions),
      modelFamilies: devinModelFamilies(devinModelChoices(setup)),
      modeId: "",
      muteUpdates: didLoad,
      cancelled: false,
      promptInFlight: false,
      runtimeMode: input.runtimeMode,
      planning: input.intent === "plan",
      onEvent: input.onEvent,
      approvals: new Map(),
      tools: new Map(),
      turns: Promise.resolve(),
    };
    liveRef.current = live;
    liveByThread.set(input.sessionId, live);
    resumeByThread.set(input.sessionId, { acpSessionId, cwd: input.cwd });
    live.onEvent({ type: "session.providerBound", providerSessionId: acpSessionId });
    live.onEvent({ type: "session.started" });
    return live;
  } catch (error) {
    rpc.close(error instanceof Error ? error : new Error(String(error)));
    // Children are keyed by thread: once stopped, a newer start may own the id.
    const superseded =
      startup.stopped &&
      (startups.has(input.sessionId) || liveByThread.has(input.sessionId));
    if (!superseded) {
      unwatchChild(input.sessionId);
      await killChild(input.sessionId).catch(() => undefined);
    }
    throw error;
  }
}

async function applyModelSelection(
  live: Live,
  input: HarnessSessionInput,
): Promise<void> {
  const nativeId = nativeModelId(input.model).trim();
  const modelId = devinModelValue(
    live.modelFamilies,
    nativeId,
    input.modelSettings,
  );
  const family = live.modelFamilies.find(
    (item) =>
      item.key === nativeId ||
      item.variants.some((variant) => variant.value === modelId),
  );
  const offered = live.modelFamilies.some((item) =>
    item.variants.some((variant) => variant.value === modelId),
  );
  if (modelId && modelId !== "default") {
    // Devin rejects ids it no longer lists (CLI updates rename models), which
    // would fail every turn of a session saved before the update.
    if (offered || live.modelFamilies.length === 0) {
      await setConfigOption(live, "model", modelId);
    } else if (live.staleModel !== modelId) {
      live.staleModel = modelId;
      live.onEvent({
        type: "status",
        text: `Devin no longer offers the model "${modelId}"; using its current model. Pick another model to change it.`,
      });
    }
  }
  for (const [settingId, value] of Object.entries(input.modelSettings ?? {})) {
    // Speed only picks a model variant; Devin has no fast config option.
    if (settingId === "fast") continue;
    if (settingId === "effort") {
      // Already spent on the variant pick (old catalogs) or, for Fusion, the
      // lead effort riding inside the model id. Newer CLIs take the rest as
      // the session-wide `thought_level` select.
      const spent =
        !!family &&
        (family.variants.some((variant) => variant.fusion) ||
          (family.variants.length > 1 &&
            family.variants.some((variant) => variant.effort)));
      if (spent) continue;
    }
    const configId = resolveSettingConfigId(live.configOptions, settingId);
    if (!configId || configId === "model" || configId === "mode") continue;
    // A value this model does not offer would fail every turn with Invalid
    // params. Efforts differ per model, so a saved one maps to the nearest
    // level the model has; anything else keeps the session's value.
    const option = live.configOptions.find((entry) => entry.id === configId);
    const next =
      option?.choices && !option.choices.includes(value)
        ? settingId === "effort"
          ? devinNearestEffort(value, option.choices)
          : null
        : value;
    if (next) await setConfigOption(live, configId, next);
  }
}

async function setConfigOption(
  live: Live,
  configId: string,
  value: string,
): Promise<void> {
  const current = live.configOptions.find((option) => option.id === configId);
  // Only options the session advertised are valid targets.
  if (!current || String(current.currentValue ?? "") === value) return;
  const result = await live.rpc.request(
    "session/set_config_option",
    { sessionId: live.acpSessionId, configId, value },
    CONTROL_TIMEOUT_MS,
  );
  const options = asRecord(result)?.configOptions;
  if (Array.isArray(options)) live.configOptions = readConfigOptions(options);
}

async function applyRuntimeMode(
  live: Live,
  runtimeMode: RuntimeMode,
  planning: boolean,
): Promise<void> {
  const modeId = devinModeId(runtimeMode, planning);
  if (modeId === live.modeId) return;
  // Fail closed: a rejected downgrade must not leave bypass active.
  await live.rpc.request(
    "session/set_mode",
    { sessionId: live.acpSessionId, modeId },
    CONTROL_TIMEOUT_MS,
  );
  live.modeId = modeId;
}

async function prompt(
  live: Live,
  blocks: ReturnType<typeof devinPromptBlocks>,
): Promise<void> {
  if (blocks.length === 0) return;
  live.promptInFlight = true;
  try {
    const result = await live.rpc.request(
      "session/prompt",
      { sessionId: live.acpSessionId, prompt: blocks },
      PROMPT_TIMEOUT_MS,
    );
    const stopReason = asRecord(result)?.stopReason;
    if (live.cancelled || stopReason === "cancelled") return;
    if (stopReason != null && stopReason !== "end_turn") {
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
    const failure = devinTurnError(error);
    live.onEvent({ type: "session.error", message: failure.message });
    throw failure;
  } finally {
    live.promptInFlight = false;
  }
}

function handleUpdate(
  threadId: string,
  live: Live | null,
  params: unknown,
  muted: boolean,
): void {
  const commands = devinCommandsFromUpdate(params);
  if (commands) {
    commandsByThread.set(threadId, commands);
    for (const listener of commandListeners) listener(threadId, commands);
    return;
  }
  const title = devinSessionTitle(params);
  if (title) {
    for (const waiter of [...(titleWaiters.get(threadId) ?? [])]) waiter(title);
    return;
  }
  if (!live) return;
  const update = asRecord(asRecord(params)?.update);
  // Config state stays fresh while muted so set_config_option is not resent.
  if (
    update?.sessionUpdate === "config_option_update" &&
    Array.isArray(update.configOptions)
  ) {
    live.configOptions = readConfigOptions(update.configOptions);
    const choices = devinModelChoices(update.configOptions);
    if (choices.length > 0) live.modelFamilies = devinModelFamilies(choices);
    return;
  }
  if (update?.sessionUpdate === "current_mode_update") {
    const mode = update.currentModeId;
    if (typeof mode === "string") live.modeId = mode as DevinModeId;
    return;
  }
  const tool = devinToolInfo(params);
  if (tool) {
    const previous = live.tools.get(tool.callId) ?? {};
    live.tools.set(tool.callId, {
      title: tool.info.title ?? previous.title,
      kind: tool.info.kind ?? previous.kind,
      preview: tool.info.preview ?? previous.preview,
    });
  }
  if (muted || live.muteUpdates) return;
  const routed = live.devinSubagents.translate(params, devinEventsFromUpdate(params));
  for (const event of live.subagents.route(routed.params, routed.events)) {
    live.onEvent(event);
  }
}

async function handlePermission(
  live: Live,
  id: JsonRpcId,
  params: unknown,
): Promise<void> {
  const request = permissionRequestFromAcp(params);
  // Devin's request names only the call id; the row was described earlier.
  const known = request.callId ? live.tools.get(request.callId) : undefined;
  const command = devinPermissionCommand(params);
  const kind = request.kind ?? known?.kind;
  const title =
    request.title !== "Permission"
      ? request.title
      : known?.title ?? (command ? `Run ${command}` : "Permission");
  const preview = request.preview ?? known?.preview;

  let optionId: string | null;
  if (
    !live.promptInFlight ||
    live.cancelled ||
    live.muteUpdates ||
    request.optionIds.length === 0
  ) {
    optionId = null;
  } else if (live.planning) {
    optionId = oneTimeOptionId(
      kind === "read" || kind === "search" ? "allow" : "deny",
      request.optionIds,
      request.optionKinds,
    );
  } else if (live.runtimeMode === "full-access") {
    optionId = permissionOptionId("allow", request.optionIds, request.optionKinds);
  } else {
    const requestId = ++approvalSeq;
    const pending = new Promise<ApprovalDecision>((resolve) => {
      live.approvals.set(requestId, resolve);
    });
    live.onEvent({
      type: "approval.requested",
      requestId,
      title,
      kind,
      callId: request.callId,
      preview,
    });
    const decision = await pending;
    live.approvals.delete(requestId);
    live.onEvent({ type: "approval.resolved", requestId, decision });
    optionId = live.cancelled
      ? null
      : oneTimeOptionId(decision, request.optionIds, request.optionKinds);
  }
  await live.rpc.respond(id, {
    outcome: optionId ? { outcome: "selected", optionId } : { outcome: "cancelled" },
  });
}

/**
 * A single approval must never grant standing access: Devin's `allow_always`
 * option is `switch_bypass`, which flips the session into bypass mode. With no
 * one-time allow on offer the request is cancelled instead.
 */
function oneTimeOptionId(
  decision: ApprovalDecision,
  optionIds: string[],
  optionKinds: Record<string, string>,
): string | null {
  if (decision !== "allow") return permissionOptionId(decision, optionIds, optionKinds);
  return (
    optionIds.find((id) => optionKinds[id] === "allow_once") ??
    optionIds.find((id) => !optionKinds[id] && (id === "allow_once" || id === "allow-once")) ??
    null
  );
}

function resolveApprovals(live: Live): void {
  for (const resolve of live.approvals.values()) resolve("deny");
  live.approvals.clear();
}

import { nativeModelId } from "../models";
import type { RuntimeMode } from "../session";
import { AcpClient, type AcpHandlers } from "./acp";
import {
  killChild,
  resolveMcodeBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "./child";
import {
  mcodeAutoPermissionOption,
  mcodeConfigToModelSettings,
  mcodeEventsFromAcpUpdate,
  mcodeExtractModelConfigId,
  mcodeModeId,
  mcodePermissionOptionId,
  mcodePermissionRequestFromAcp,
  mcodePromptBlocks,
  mcodeReadConfigOptions,
  mcodeResolveSettingConfigId,
  mcodeSessionIdFromResult,
  type SessionConfigOption,
} from "./mcodeProtocol";
import type {
  ApprovalDecision,
  HarnessEvent,
  SendTurnInput,
  SteerTurnInput,
} from "./types";

type SessionSetupResult = {
  sessionId?: string;
  session_id?: string;
  configOptions?: unknown;
};

type Live = {
  acp: AcpClient;
  acpSessionId: string;
  cwd: string;
  modelConfigId: string;
  configOptions: SessionConfigOption[];
  muteUpdates: boolean;
  cancelled: boolean;
  runtimeMode: RuntimeMode;
  planning: boolean;
  onEvent: (event: HarnessEvent) => void;
  turns: Promise<void>;
};

type Resume = {
  acpSessionId: string;
  cwd: string;
};

const INIT_TIMEOUT_MS = 12_000;
const SESSION_TIMEOUT_MS = 45_000;
const CONTROL_TIMEOUT_MS = 15_000;
const PROMPT_TIMEOUT_MS = 30 * 60_000;

const AUTH_HELP =
  "mcode has no MiniMax credential it can read from here. " +
  "Run `mcode login` in a terminal (or set the MINIMAX_API_KEY / use a configured provider), then retry.";

function mcodeStartupError(error: unknown): Error {
  const detail = error instanceof Error ? error.message : String(error);
  if (/credential|login|api key|provider/i.test(detail)) {
    return new Error(`${detail.trim()}\n\n${AUTH_HELP}`);
  }
  if (/timed out/i.test(detail)) {
    return new Error(
      `mcode did not answer initialize within ${INIT_TIMEOUT_MS / 1000}s. ${AUTH_HELP}`,
    );
  }
  return new Error(`mcode did not start. ${detail}`);
}

const CLIENT_CAPABILITIES = {
  fs: { readTextFile: false, writeTextFile: false },
  terminal: false,
};

const liveByThread = new Map<string, Live>();
const resumeByThread = new Map<string, Resume>();
const cancelledThreads = new Set<string>();

/**
 * Live mcode adapter. Spawns `mcode acp` and talks Agent Client Protocol.
 * Mirrors the fx adapter shape; difference is that mcode does not accept a
 * --model spawn flag — model selection is set via session/set_config_option
 * once the session is up.
 */
export async function sendMcodeTurn(input: SendTurnInput): Promise<void> {
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
        await applyRuntimeMode(
          live,
          input.runtimeMode,
          input.intent === "plan",
        );
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
      await stopMcodeSession(input.sessionId);
    }
    throw error;
  }
}

export async function steerMcodeTurn(_input: SteerTurnInput): Promise<void> {
  throw new Error("mcode does not support steering an in-flight turn");
}

/** mcode auto-approves in `code` mode, so there is never a pending approval. */
export function respondMcodeApproval(
  _sessionId: string,
  _requestId: number,
  _decision: ApprovalDecision,
) {}

export async function cancelMcodeTurn(sessionId: string): Promise<void> {
  const live = liveByThread.get(sessionId);
  if (!live) {
    cancelledThreads.add(sessionId);
    return;
  }
  live.cancelled = true;
  live.muteUpdates = true;
  await live.acp
    .notify("session/cancel", { sessionId: live.acpSessionId })
    .catch(() => undefined);
  live.acp.rejectPending(new Error("cancelled"));
}

export async function stopMcodeSession(sessionId: string): Promise<void> {
  cancelledThreads.delete(sessionId);
  const live = liveByThread.get(sessionId);
  liveByThread.delete(sessionId);
  if (live) live.muteUpdates = true;
  live?.acp.close();
  unwatchChild(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

export async function forgetMcodeSession(sessionId: string): Promise<void> {
  resumeByThread.delete(sessionId);
  await stopMcodeSession(sessionId);
}

export function bindMcodeSession(
  threadId: string,
  acpSessionId: string,
  cwd: string,
): void {
  const sessionId = acpSessionId.trim();
  if (!threadId || !sessionId || !cwd.trim()) return;
  resumeByThread.set(threadId, { acpSessionId: sessionId, cwd });
}

async function ensureLive(input: SendTurnInput): Promise<Live> {
  const existing = liveByThread.get(input.sessionId);
  if (existing && existing.cwd === input.cwd) {
    existing.onEvent = input.onEvent;
    existing.runtimeMode = input.runtimeMode;
    return existing;
  }
  if (existing) {
    resumeByThread.delete(input.sessionId);
    await stopMcodeSession(input.sessionId);
  }

  const resume = resumeByThread.get(input.sessionId);
  const canLoad = resume != null && resume.cwd === input.cwd;
  if (resume && resume.cwd !== input.cwd) {
    resumeByThread.delete(input.sessionId);
  }

  const { path } = await resolveMcodeBinary();
  const handlers: AcpHandlers = {};
  const acp = new AcpClient(input.sessionId, handlers);
  const liveRef: { current: Live | null } = { current: null };
  const muteGate = { current: false };

  handlers.onNotification = (method, params) => {
    if (muteGate.current) return;
    const live = liveRef.current;
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
      acp.close(new Error("mcode exited"));
      liveByThread.delete(input.sessionId);
      emit({ type: "session.ended", code });
    },
    (line) => {
      console.debug("[monocode] mcode stderr", line);
      if (/login|credential|api key|provider/i.test(line)) {
        emit({ type: "session.error", message: line.trim() });
      }
    },
  );

  await spawnChild(input.sessionId, path, mcodeSpawnArgs(), input.cwd);

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
      throw mcodeStartupError(error);
    }

    let setup: SessionSetupResult | undefined;
    let acpSessionId: string | undefined;
    let didLoad = false;

    if (canLoad && resume) {
      try {
        setup = await acp.request<SessionSetupResult>(
          "session/resume",
          { sessionId: resume.acpSessionId },
          SESSION_TIMEOUT_MS,
        );
        acpSessionId = mcodeSessionIdFromResult(setup) ?? resume.acpSessionId;
        didLoad = true;
      } catch {
        muteGate.current = true;
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
          acpSessionId = mcodeSessionIdFromResult(setup) ?? resume.acpSessionId;
          didLoad = true;
        } catch {
          setup = undefined;
          acpSessionId = undefined;
          didLoad = false;
        } finally {
          muteGate.current = false;
        }
      }
    }

    if (!acpSessionId) {
      setup = await acp.request<SessionSetupResult>(
        "session/new",
        { cwd: input.cwd, mcpServers: [] },
        SESSION_TIMEOUT_MS,
      );
      acpSessionId = mcodeSessionIdFromResult(setup);
    }
    if (!acpSessionId) throw new Error("mcode did not return a session id");

    const configOptions = mcodeReadConfigOptions(setup?.configOptions);
    const live: Live = {
      acp,
      acpSessionId,
      cwd: input.cwd,
      modelConfigId: mcodeExtractModelConfigId(configOptions),
      configOptions,
      muteUpdates: didLoad,
      cancelled: false,
      runtimeMode: input.runtimeMode,
      planning: input.intent === "plan",
      onEvent: input.onEvent,
      turns: Promise.resolve(),
    };
    liveRef.current = live;
    liveByThread.set(input.sessionId, live);
    resumeByThread.set(input.sessionId, {
      acpSessionId,
      cwd: input.cwd,
    });
    live.onEvent({
      type: "session.providerBound",
      providerSessionId: acpSessionId,
    });
    live.onEvent({ type: "session.started" });
    return live;
  } catch (error) {
    acp.close(error instanceof Error ? error : new Error(String(error)));
    await stopMcodeSession(input.sessionId);
    throw error;
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

  await setConfigOption(live, modelConfigId, base).catch((error: unknown) => {
    ignoreUnsupportedControl("set_config_option", error);
  });
  if (modelConfigId !== "model") {
    await setConfigOption(live, "model", base).catch((error: unknown) => {
      ignoreUnsupportedControl("set_config_option", error);
    });
  }

  for (const [settingId, value] of Object.entries(settings)) {
    const configId = mcodeResolveSettingConfigId(live.configOptions, settingId);
    if (!configId || configId === "provider") continue;
    await setConfigOption(live, configId, value).catch((error: unknown) => {
      ignoreUnsupportedControl("set_config_option", error);
    });
  }
}

async function applyRuntimeMode(
  live: Live,
  runtimeMode: RuntimeMode,
  planning = false,
): Promise<void> {
  await live.acp
    .request(
      "session/set_mode",
      {
        sessionId: live.acpSessionId,
        modeId: planning ? "ask" : mcodeModeId(runtimeMode),
      },
      CONTROL_TIMEOUT_MS,
    )
    .catch((error: unknown) => {
      ignoreUnsupportedControl("set_mode", error);
    });
}

async function setConfigOption(
  live: Live,
  configId: string,
  value: string | boolean,
): Promise<void> {
  const encoded = String(value);
  const current = live.configOptions.find((option) => option.id === configId);
  if (current && String(current.currentValue ?? "") === encoded) return;

  const result = await live.acp.request<SessionSetupResult>(
    "session/set_config_option",
    {
      sessionId: live.acpSessionId,
      configId,
      value: encoded,
    },
    CONTROL_TIMEOUT_MS,
  );
  if (result?.configOptions) {
    live.configOptions = mcodeReadConfigOptions(result.configOptions);
    live.modelConfigId = mcodeExtractModelConfigId(live.configOptions);
  }
}

function mcodeSpawnArgs(): string[] {
  // mcode takes no model flag on `acp`; the model is set per-session via
  // session/set_config_option. The plan/build/etc. mode is set the same way.
  return ["acp"];
}

async function prompt(live: Live, input: SendTurnInput): Promise<void> {
  try {
    const blocks = mcodePromptBlocks(input.text);
    if (blocks.length === 0) return;
    await live.acp.request(
      "session/prompt",
      {
        sessionId: live.acpSessionId,
        prompt: blocks,
      },
      PROMPT_TIMEOUT_MS,
    );
    if (live.cancelled) return;
    live.onEvent({ type: "message.completed" });
    live.onEvent({ type: "reasoning.completed" });
  } catch (error) {
    if (live.cancelled) return;
    const detail = error instanceof Error ? error.message : String(error);
    live.onEvent({
      type: "session.error",
      message: /credential|login|api key|provider/i.test(detail)
        ? `${detail.trim()}\n\n${AUTH_HELP}`
        : detail,
    });
    throw error;
  }
}

function ignoreUnsupportedControl(method: string, error: unknown): void {
  console.debug(`[monocode] mcode ${method} failed`, error);
  const detail = error instanceof Error ? error.message : String(error);
  if (/timed out|not running|exited|closed|pipe/i.test(detail)) throw error;
}

function handleNotification(live: Live, method: string, params: unknown) {
  if (method !== "session/update") return;
  for (const event of mcodeEventsFromAcpUpdate(params)) {
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
  await live.acp
    .respondError(id, {
      code: -32601,
      message: `Method not found: ${method}`,
    })
    .catch(() => undefined);
}

async function handlePermission(live: Live, id: number, params: unknown) {
  const request = mcodePermissionRequestFromAcp(params);
  if (request.callId) {
    live.onEvent({
      type: "tool.updated",
      callId: request.callId,
      title: request.title,
      kind: request.kind,
      preview: request.preview,
    });
  }
  const optionId = live.planning
    ? mcodePermissionOptionId(
        request.kind === "read" || request.kind === "search" ? "allow" : "deny",
        request.optionIds,
      )
    : (mcodeAutoPermissionOption(live.runtimeMode, request.optionIds) ??
      mcodePermissionOptionId("allow", request.optionIds));
  await live.acp.respond(id, {
    outcome: { outcome: "selected", optionId },
  });
}

export { mcodeConfigToModelSettings };

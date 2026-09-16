import { nativeModelId } from "../models";
import { AcpSubagents } from "./acpSubagents";
import type { RuntimeMode } from "../session";
import { AcpClient, type AcpHandlers } from "./acp";
import {
  killChild,
  resolveKimiBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "./child";
import {
  autoPermissionOption,
  asRecord,
  eventsFromAcpUpdate,
  extractModelConfigId,
  kimiModeId,
  kimiPromptBlocks,
  permissionOptionId,
  permissionRequestFromAcp,
  readConfigOptions,
  resolveSettingConfigId,
  sessionIdFromResult,
  type SessionConfigOption,
} from "./kimiProtocol";
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
  subagents: AcpSubagents;
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
  approvals: Map<number, (decision: ApprovalDecision) => void>;
  turns: Promise<void>;
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

const AUTH_HELP = "Run `kimi login` in Terminal to sign in to Kimi Code.";

function kimiError(error: unknown): Error {
  const detail = error instanceof Error ? error.message : String(error);
  return new Error(/auth|login|sign.in|credential|api.key|timed out/i.test(detail)
    ? `${detail.trim()}\n\n${AUTH_HELP}` : detail);
}

const CLIENT_CAPABILITIES = {
  fs: { readTextFile: false, writeTextFile: false },
  terminal: false,
};

const liveByThread = new Map<string, Live>();
const resumeByThread = new Map<string, Resume>();
const cancelledThreads = new Set<string>();

/** Live Kimi Code adapter. Spawns `kimi acp` and talks ACP over stdio. */
export async function sendKimiTurn(input: SendTurnInput): Promise<void> {
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
    // A timed-out or failed turn leaves the process state unknowable. Keep
    // its provider session id, but recycle the child so the next turn can
    // resume instead of inheriting a permanently wedged transport.
    if (liveByThread.get(input.sessionId) === live) {
      await stopKimiSession(input.sessionId);
    }
    throw error;
  }
}

export async function steerKimiTurn(_input: SteerTurnInput): Promise<void> {
  throw new Error("Kimi Code does not support steering an in-flight turn");
}

export function respondKimiApproval(
  sessionId: string,
  requestId: number,
  decision: ApprovalDecision,
) {
  liveByThread.get(sessionId)?.approvals.get(requestId)?.(decision);
}

export async function cancelKimiTurn(sessionId: string): Promise<void> {
  const live = liveByThread.get(sessionId);
  if (!live) {
    cancelledThreads.add(sessionId);
    return;
  }
  live.cancelled = true;
  live.muteUpdates = true;
  for (const resolve of live.approvals.values()) resolve("deny");
  live.approvals.clear();
  await live.acp
    .notify("session/cancel", { sessionId: live.acpSessionId })
    .catch(() => undefined);
  live.acp.rejectPending(new Error("cancelled"));
}

export async function stopKimiSession(sessionId: string): Promise<void> {
  cancelledThreads.delete(sessionId);
  const live = liveByThread.get(sessionId);
  liveByThread.delete(sessionId);
  if (live) {
    live.cancelled = true;
    live.muteUpdates = true;
    for (const resolve of live.approvals.values()) resolve("deny");
    live.approvals.clear();
  }
  live?.acp.close();
  unwatchChild(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

export async function forgetKimiSession(sessionId: string): Promise<void> {
  resumeByThread.delete(sessionId);
  await stopKimiSession(sessionId);
}

export function bindKimiSession(
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
    await stopKimiSession(input.sessionId);
  }

  const resume = resumeByThread.get(input.sessionId);
  const canLoad = resume != null && resume.cwd === input.cwd;
  if (resume && resume.cwd !== input.cwd) {
    resumeByThread.delete(input.sessionId);
  }

  const { path } = await resolveKimiBinary();
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
    void handleRequest(live, id, method, params).catch(() => undefined);
  };

  // ensureLive runs once per session, so these handlers outlive the turn that
  // created them. Routing through the live record keeps them on the *current*
  // turn's listener — capturing `input.onEvent` meant every exit and stderr
  // error after turn 1 was addressed to a finished turn and silently dropped,
  // leaving the session spinning on "Working…" forever.
  const emit = (event: HarnessEvent) => {
    (liveRef.current?.onEvent ?? input.onEvent)(event);
  };

  watchChild(
    input.sessionId,
    (line) => acp.pushLine(line),
    (code) => {
      acp.close(new Error("Kimi Code exited"));
      liveByThread.delete(input.sessionId);
      emit({ type: "session.ended", code });
    },
    (line) => {
      console.debug("[monocode] kimi stderr", line);
    },
  );

  try {
    await spawnChild(input.sessionId, path, ["acp"], input.cwd);
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
      throw kimiError(error);
    }

    let setup: SessionSetupResult | undefined;
    let acpSessionId: string | undefined;
    let didLoad = false;

    if (canLoad && resume) {
      try {
        setup = await acp.request<SessionSetupResult>(
          "session/resume",
          { sessionId: resume.acpSessionId, cwd: input.cwd, mcpServers: [] },
          SESSION_TIMEOUT_MS,
        );
        acpSessionId = sessionIdFromResult(setup) ?? resume.acpSessionId;
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
          acpSessionId = sessionIdFromResult(setup) ?? resume.acpSessionId;
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
      acpSessionId = sessionIdFromResult(setup);
    }
    if (!acpSessionId) throw new Error("Kimi Code did not return a session id");

    const configOptions = readConfigOptions(setup?.configOptions);
    const live: Live = {
      subagents: new AcpSubagents(),
      acp,
      acpSessionId,
      cwd: input.cwd,
      modelConfigId: extractModelConfigId(configOptions),
      configOptions,
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
    await stopKimiSession(input.sessionId);
    throw kimiError(error);
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
        modeId: kimiModeId(runtimeMode, planning),
      },
      CONTROL_TIMEOUT_MS,
    );
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
    live.configOptions = readConfigOptions(result.configOptions);
    live.modelConfigId = extractModelConfigId(live.configOptions);
  }
}

async function prompt(live: Live, input: SendTurnInput): Promise<void> {
  try {
    const blocks = kimiPromptBlocks(input.text, input.attachments);
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
    const failure = kimiError(error);
    live.onEvent({
      type: "session.error",
      message: failure.message,
    });
    throw failure;
  }
}

function handleNotification(live: Live, method: string, params: unknown) {
  if (method !== "session/update") return;
  const rec = asRecord(params);
  const update = asRecord(rec?.update) ?? rec;
  if (update?.sessionUpdate === "config_option_update") {
    live.configOptions = readConfigOptions(update.configOptions);
    live.modelConfigId = extractModelConfigId(live.configOptions);
  }
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
  await live.acp
    .respondError(id, {
      code: -32601,
      message: `Method not found: ${method}`,
    })
    .catch(() => undefined);
}

async function handlePermission(live: Live, id: number, params: unknown) {
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
  let optionId: string | null;
  if (live.cancelled || live.muteUpdates || request.optionIds.length === 0) {
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

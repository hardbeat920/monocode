import { nativeModelId, setHarnessModels } from "../models";
import { openCrabsPromptBlocks } from "./opencrabsPrompt";
import type { RuntimeMode } from "../session";
import { AcpClient, type AcpHandlers } from "./acp";
import {
  killChild,
  resolveOpenCrabsBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "./child";
import {
  autoPermissionOption,
  eventsFromAcpUpdate,
  permissionOptionId,
  permissionRequestFromAcp,
  modelsFromSessionNew,
  sessionIdFromResult,
} from "./opencrabsProtocol";
import type {
  ApprovalDecision,
  HarnessEvent,
  SendTurnInput,
  SteerTurnInput,
} from "./types";

type Live = {
  acp: AcpClient;
  acpSessionId: string;
  cwd: string;
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

// OpenCrabs boots a full runtime (config, brain files, provider handshake)
// before it can answer `initialize`, so give it more room than a thin CLI.
const INIT_TIMEOUT_MS = 30_000;
const SESSION_TIMEOUT_MS = 45_000;
const CONTROL_TIMEOUT_MS = 15_000;
const PROMPT_TIMEOUT_MS = 30 * 60_000;

const SERVER_HELP =
  "The OpenCrabs ACP server mode is paired work that may not be released yet. " +
  "Check that your opencrabs build supports `opencrabs acp`.";

const CLIENT_CAPABILITIES = {
  fs: { readTextFile: false, writeTextFile: false },
  terminal: false,
};

const liveByThread = new Map<string, Live>();
const resumeByThread = new Map<string, Resume>();
const cancelledThreads = new Set<string>();

/**
 * Live OpenCrabs adapter. Spawns `opencrabs acp` and talks Agent Client
 * Protocol over stdio. Permission requests surface in the UI unless the
 * runtime mode auto-answers them.
 */
export async function sendOpenCrabsTurn(input: SendTurnInput): Promise<void> {
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
        await applyRuntimeMode(live, input);
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
    // A timed-out or failed turn leaves the child's protocol state unknowable.
    // Keep the provider session id, but recycle the process so the next turn
    // resumes on a fresh transport instead of a wedged one.
    if (liveByThread.get(input.sessionId) === live) {
      await stopOpenCrabsSession(input.sessionId);
    }
    throw error;
  }
}

export async function steerOpenCrabsTurn(input: SteerTurnInput): Promise<void> {
  const live = liveByThread.get(input.sessionId);
  if (!live) throw new Error("No active OpenCrabs session");
  const blocks = await openCrabsPromptBlocks(input.text, input.attachments);
  if (blocks.length === 0) return;
  await live.acp
    .notify("session/steer", {
      sessionId: live.acpSessionId,
      prompt: blocks,
    })
    .catch(() => undefined);
}

export function respondOpenCrabsApproval(
  sessionId: string,
  requestId: number,
  decision: ApprovalDecision,
): void {
  liveByThread.get(sessionId)?.approvals.get(requestId)?.(decision);
}

/** Abort the in-flight prompt without tearing down the ACP session. */
export async function cancelOpenCrabsTurn(sessionId: string): Promise<void> {
  const live = liveByThread.get(sessionId);
  if (!live) {
    cancelledThreads.add(sessionId);
    return;
  }
  live.cancelled = true;
  live.muteUpdates = true;
  for (const [, resolve] of live.approvals) resolve("deny");
  live.approvals.clear();
  await live.acp
    .notify("session/cancel", { sessionId: live.acpSessionId })
    .catch(() => undefined);
  live.acp.rejectPending(new Error("cancelled"));
}

/** Kill the process but keep the ACP session id so we can session/load. */
export async function stopOpenCrabsSession(sessionId: string): Promise<void> {
  cancelledThreads.delete(sessionId);
  const live = liveByThread.get(sessionId);
  liveByThread.delete(sessionId);
  if (live) {
    live.muteUpdates = true;
    for (const [, resolve] of live.approvals) resolve("deny");
    live.approvals.clear();
  }
  live?.acp.close();
  unwatchChild(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

/** Delete or idle detach — drop the OpenCrabs conversation too. */
export async function forgetOpenCrabsSession(sessionId: string): Promise<void> {
  resumeByThread.delete(sessionId);
  await stopOpenCrabsSession(sessionId);
}

/** Seed ACP resume state for a restored MonoCode session. */
export function bindOpenCrabsSession(
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
    existing.planning = input.intent === "plan";
    return existing;
  }
  if (existing) {
    resumeByThread.delete(input.sessionId);
    await stopOpenCrabsSession(input.sessionId);
  }

  const resume = resumeByThread.get(input.sessionId);
  const canLoad = resume != null && resume.cwd === input.cwd;
  if (resume && resume.cwd !== input.cwd) {
    resumeByThread.delete(input.sessionId);
  }

  const { path } = await resolveOpenCrabsBinary();
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

  // ensureLive runs once per session, so these handlers outlive the turn that
  // created them. Route through liveRef so events after turn 1 reach the
  // current turn's listener instead of a finished one.
  const emit = (event: HarnessEvent) => {
    (liveRef.current?.onEvent ?? input.onEvent)(event);
  };

  watchChild(
    input.sessionId,
    (line) => acp.pushLine(line),
    (code) => {
      acp.close(new Error("opencrabs exited"));
      liveByThread.delete(input.sessionId);
      emit({ type: "session.ended", code });
    },
    (line) => {
      console.debug("[monocode] opencrabs stderr", line);
    },
  );

  await spawnChild(input.sessionId, path, spawnArgs(input.model), input.cwd);

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

    let setup: unknown;
    let acpSessionId: string | undefined;
    let didLoad = false;

    if (canLoad && resume) {
      muteGate.current = true;
      try {
        setup = await acp.request(
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

    if (!acpSessionId) {
      setup = await acp.request(
        "session/new",
        { cwd: input.cwd, mcpServers: [] },
        SESSION_TIMEOUT_MS,
      );
      acpSessionId = sessionIdFromResult(setup);
    }
    if (!acpSessionId) throw new Error("opencrabs did not return a session id");

    // Live catalog: replace the static "default" picker entry with the
    // server's configured provider/model pairs.
    const catalog = modelsFromSessionNew(setup);
    if (catalog.available.length > 0) {
      setHarnessModels(
        "opencrabs",
        catalog.available.map((entry) => ({
          id: `opencrabs:${entry.modelId}`,
          harness: "opencrabs" as const,
          name: entry.name,
          nativeId: entry.modelId,
        })),
      );
    }

    const live: Live = {
      acp,
      acpSessionId,
      cwd: input.cwd,
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
    await stopOpenCrabsSession(input.sessionId);
    throw error;
  }
}

/**
 * Model selection is best-effort: the static catalog ships only `default`
 * (empty native id, skipped here), while live catalog entries carry
 * `provider/model` pairs the server routes through `session/set_model`.
 */
async function applyModelSelection(
  live: Live,
  input: SendTurnInput,
): Promise<void> {
  const base = nativeModelId(input.model).trim();
  if (!base) return;
  await live.acp
    .request(
      "session/set_model",
      { sessionId: live.acpSessionId, modelId: base },
      CONTROL_TIMEOUT_MS,
    )
    .catch(() => undefined);
}

function spawnArgs(model: string): string[] {
  const native = nativeModelId(model).trim();
  return native ? ["acp", "--model", native] : ["acp"];
}

/**
 * Push the runtime/plan mode server-side so the approval policy lives where
 * the tools run. Client-side gating in handlePermission stays as backstop,
 * and an older binary without set_mode support degrades to it.
 */
async function applyRuntimeMode(
  live: Live,
  input: SendTurnInput,
): Promise<void> {
  const modeId = input.intent === "plan" ? "plan" : input.runtimeMode;
  await live.acp
    .request(
      "session/set_mode",
      { sessionId: live.acpSessionId, modeId },
      CONTROL_TIMEOUT_MS,
    )
    .catch((error: unknown) => {
      const detail = error instanceof Error ? error.message : String(error);
      console.debug("[monocode] opencrabs set_mode failed", detail);
      if (/timed out|not running|exited|closed|pipe/i.test(detail)) throw error;
    });
}

async function prompt(live: Live, input: SendTurnInput): Promise<void> {
  try {
    const blocks = await openCrabsPromptBlocks(input.text, input.attachments);
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
      message: /timed out|not running|exited|closed|pipe|method not found/i.test(
        detail,
      )
        ? `${detail.trim()}\n\n${SERVER_HELP}`
        : detail,
    });
    throw error;
  }
}

function handleNotification(live: Live, method: string, params: unknown) {
  if (method !== "session/update") return;
  for (const event of eventsFromAcpUpdate(params)) {
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

  if (live.planning) {
    const normalized = (request.kind ?? "").toLowerCase();
    const readOnly = normalized === "read" || normalized === "search";
    await live.acp.respond(id, {
      outcome: {
        outcome: "selected",
        optionId: permissionOptionId(readOnly ? "allow" : "deny", request.optionIds),
      },
    });
    return;
  }

  const auto = autoPermissionOption(
    live.runtimeMode,
    request.kind,
    request.optionIds,
  );
  if (auto) {
    await live.acp.respond(id, {
      outcome: { outcome: "selected", optionId: auto },
    });
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

  await live.acp.respond(id, {
    outcome: {
      outcome: "selected",
      optionId: permissionOptionId(decision, request.optionIds),
    },
  });
}

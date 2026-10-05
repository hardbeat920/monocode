import {
  nativeModelId,
  setHarnessModels,
} from "../../../../features/sessions/model/models";
import type { RuntimeMode } from "../../../../features/sessions/model/session";
import { AcpClient, type AcpHandlers } from "../../core/acp";
import {
  killChild,
  resolveOpenCrabsBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import {
  eventsFromAcpUpdate,
  modelsFromSessionNew,
  nativeCommandsFromUpdate,
  sessionIdFromResult,
} from "./opencrabsProtocol";
import { AcpSubagents } from "../../core/acpSubagents";
import { handlePermission } from "./opencrabsApproval";
import {
  cacheNativeCommands,
  clearNativeCommands,
} from "./opencrabsCommands";
import type {
  ApprovalDecision,
  HarnessEvent,
  SendTurnInput,
} from "../../core/types";

/**
 * Session lifecycle for the OpenCrabs adapter: process spawn, ACP
 * handshake, resume, and teardown. Turn orchestration lives in
 * `opencrabs.ts`; this module owns the machinery that keeps a live child
 * and its ACP session id alive across turns.
 */

export type Live = {
  acp: AcpClient;
  acpSessionId: string;
  threadId: string;
  cwd: string;
  muteUpdates: boolean;
  cancelled: boolean;
  runtimeMode: RuntimeMode;
  planning: boolean;
  subagents: AcpSubagents;
  onEvent: (event: HarnessEvent) => void;
  approvals: Map<number, (decision: ApprovalDecision) => void>;
  turns: Promise<void>;
};

export type Resume = {
  acpSessionId: string;
  cwd: string;
};

// OpenCrabs boots a full runtime (config, brain files, provider handshake)
// before it can answer `initialize`, so give it more room than a thin CLI.
export const INIT_TIMEOUT_MS = 30_000;
export const SESSION_TIMEOUT_MS = 45_000;
export const CONTROL_TIMEOUT_MS = 15_000;
export const PROMPT_TIMEOUT_MS = 30 * 60_000;

export const SERVER_HELP =
  "The ACP server mode ships in opencrabs v0.5.2 and later. " +
  "Check `opencrabs --version`, then upgrade (or point the resolver at a " +
  "newer binary) and retry.";

const CLIENT_CAPABILITIES = {
  fs: { readTextFile: false, writeTextFile: false },
  terminal: false,
};

export const liveByThread = new Map<string, Live>();
export const resumeByThread = new Map<string, Resume>();
export const cancelledThreads = new Set<string>();

export async function ensureLive(input: SendTurnInput): Promise<Live> {
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
    // Control-plane carve-out: the commands push can land during the
    // session/load window — before `live` exists and while transcript replay
    // is muted. Muting exists to keep replay out of the transcript; it must
    // not drop command discovery, or every restarted session loses the
    // autocomplete catalog.
    const pushedCommands = nativeCommandsFromUpdate(params);
    if (pushedCommands) {
      cacheNativeCommands(input.sessionId, pushedCommands);
      return;
    }
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

  await spawnChild(
    input.sessionId,
    path,
    spawnArgs(input.model),
    input.cwd,
    undefined,
    "opencrabs",
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

    let setup: unknown;
    let acpSessionId: string | undefined;
    let resumeFailureReason: string | undefined;

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
      } catch (error) {
        // Context loss must be visible: MonoCode renders the old transcript
        // locally, but a fresh opencrabs session has no memory of it. The
        // notice is deferred until the replacement session is confirmed —
        // saying "started a fresh session" before session/new succeeds
        // would claim a fallback that may never exist.
        resumeFailureReason = error instanceof Error ? error.message : String(error);
        setup = undefined;
        acpSessionId = undefined;
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
    if (resumeFailureReason) {
      // The fallback session exists: now the boundary notice is true.
      emit({
        type: "interjection",
        customType: "custom",
        text: `OpenCrabs session could not be resumed (${resumeFailureReason}) — started a fresh session. Earlier messages in this transcript are no longer in the agent's context.`,
      });
    }

    // Live catalog: replace the static "Configured model" picker entry with
    // the server's configured provider/model pairs.
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
      threadId: input.sessionId,
      cwd: input.cwd,
      // Never mute a resumed session: the load-window replay is already
      // dropped (live is null and muteGate is closed until the response
      // resolves; NDJSON ordering guarantees no stragglers). Unsolicited
      // pushes after load are the cross-surface mirror — the whole point.
      muteUpdates: false,
      cancelled: false,
      runtimeMode: input.runtimeMode,
      planning: input.intent === "plan",
      subagents: new AcpSubagents(),
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
    // Reflect the server's current model in the thread badge — on load this
    // is the restored per-session pick, so the picker survives restarts.
    if (catalog.current) {
      live.onEvent({
        type: "session.configChanged",
        model: `opencrabs:${catalog.current}`,
      });
    }
    live.onEvent({ type: "session.started" });
    return live;
  } catch (error) {
    acp.close(error instanceof Error ? error : new Error(String(error)));
    await stopOpenCrabsSession(input.sessionId);
    // A bare "initialize timed out" names no harness and suggests nothing.
    // The connect path is the one place the transcript error line cannot
    // say who dropped the ball — name it and hand over the recovery lever.
    const detail = error instanceof Error ? error.message : String(error);
    throw new Error(
      `OpenCrabs failed to connect: ${detail.trim()}\n\n${SERVER_HELP}`,
    );
  }
}

export function spawnArgs(model: string): string[] {
  const native = nativeModelId(model).trim();
  // "default" is the placeholder id from the static catalog, not a model the
  // server knows — spawning `--model default` only works by fallback luck.
  // Omit the flag so the server uses its configured default model.
  return native && native.toLowerCase() !== "default"
    ? ["acp", "--model", native]
    : ["acp"];
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
  clearNativeCommands(sessionId);
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

function handleNotification(live: Live, method: string, params: unknown) {
  if (method !== "session/update") return;
  const commands = nativeCommandsFromUpdate(params);
  if (commands) {
    cacheNativeCommands(live.threadId, commands);
    return;
  }
  for (const event of live.subagents.route(
    params,
    eventsFromAcpUpdate(params),
  )) {
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

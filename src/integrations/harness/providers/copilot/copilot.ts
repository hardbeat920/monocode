import { nativeModelId } from "../../../../features/sessions/model/models";
import { promptBlocks } from "../../../../features/sessions/model/attachments";
import { nativeCommandPrompt } from "../../core/nativeCommands";
import type { NativeCommand } from "../../core/nativeCommands";
import { CopilotEvents } from "./copilotEvents";
import { JsonRpcClient, type JsonRpcId } from "../../core/jsonRpc";
import {
  killChild,
  resolveCopilotBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import type {
  ApprovalDecision,
  CompactContextInput,
  SendTurnInput,
  SteerTurnInput,
} from "../../core/types";
import {
  permissionRequestFromAcp,
  sessionIdFromResult,
} from "../grok/grokProtocol";
import {
  commandsFromCopilotUpdate,
  COPILOT_INITIALIZE_PARAMS,
  copilotConfigOptions,
  copilotError,
  copilotModeId,
  copilotPermissionOption,
  copilotSpawnArgs,
} from "./copilotProtocol";

type Live = {
  childKey: string;
  acp: JsonRpcClient;
  providerId: string;
  cwd: string;
  modeId?: string;
  modelId?: string;
  effort?: string;
  defaultEffort?: string;
  effortOptions: string[];
  muted: boolean;
  input: SendTurnInput;
  updates: CopilotEvents;
  approvals: Map<number, (decision: ApprovalDecision) => void>;
};
type Thread = {
  epoch: number;
  queue: Promise<void>;
  live?: Live;
  resume?: { providerId: string; cwd: string };
  commands: NativeCommand[];
  listeners: Set<(commands: NativeCommand[]) => void>;
};
const threads = new Map<string, Thread>();
let childSequence = 0;
let approvalSequence = 0;

class EffortValidationError extends Error {}

function threadFor(id: string): Thread {
  let thread = threads.get(id);
  if (!thread) {
    thread = {
      epoch: 0,
      queue: Promise.resolve(),
      commands: [],
      listeners: new Set(),
    };
    threads.set(id, thread);
  }
  return thread;
}

/** Serialize app-owned turns on the live ACP session. */
export async function sendCopilotTurn(input: SendTurnInput): Promise<void> {
  const thread = threadFor(input.sessionId);
  const epoch = thread.epoch;
  const cancelled = () => thread.epoch !== epoch;
  const run = thread.queue
    .catch(() => undefined)
    .then(async () => {
      if (cancelled()) return;
      try {
        const live = await ensureLive(thread, input, cancelled);
        if (!live || cancelled()) return;
        live.input = input;
        live.muted = true;
        const modeId = copilotModeId(
          input.intent === "plan" ? "plan" : "agent",
        );
        if (live.modeId !== modeId) {
          await live.acp.request(
            "session/set_mode",
            { sessionId: live.providerId, modeId },
            15_000,
          );
          live.modeId = modeId;
        }
        if (cancelled()) return;
        const modelId = nativeModelId(input.model);
        if (modelId && modelId !== "default" && live.modelId !== modelId) {
          resetEffort(live);
          // Config notifications may arrive before this RPC resolves. Ingest
          // their model/effort together, then confirm modelId after the response.
          await live.acp.request(
            "session/set_model",
            { sessionId: live.providerId, modelId },
            15_000,
          );
          live.modelId = modelId;
        }
        if (cancelled()) return;
        // Resumed sessions and native commands can enable allow-all. Reset it
        // every turn so tool requests still pass through the app's approval policy.
        // Failure must be fatal: continuing could silently bypass approvals.
        const configured = await live.acp.request(
          "session/set_config_option",
          {
            sessionId: live.providerId,
            configId: "allow_all",
            value: "off",
          },
          15_000,
        );
        updateConfiguration(live, configured);
        if (cancelled()) return;
        // ACP advertises model-specific effort choices; no child restart is needed.
        const effort =
          input.modelSettings?.reasoningEffort ?? live.defaultEffort;
        // A model without this option ignores stale/global effort preferences.
        if (effort && live.effortOptions.length > 0) {
          if (!live.effortOptions.includes(effort))
            throw new EffortValidationError(
              `Copilot does not support reasoning effort "${effort}" for this model`,
            );
          if (live.effort !== effort) {
            const result = await live.acp.request(
              "session/set_config_option",
              {
                sessionId: live.providerId,
                configId: "reasoning_effort",
                value: effort,
              },
              15_000,
            );
            updateConfiguration(live, result);
            live.effort = effort;
          }
        }
        if (cancelled()) return;
        live.muted = false;
        const blocks = promptBlocks(
          nativeCommandPrompt("copilot", input.text),
          input.attachments,
        );
        if (blocks.length === 0) return;
        const pending = live.acp.request(
          "session/prompt",
          { sessionId: live.providerId, prompt: blocks },
          30 * 60_000,
        );
        input.onAccepted?.();
        try {
          await pending;
        } finally {
          if (!cancelled()) {
            for (const event of live.updates.flush()) input.onEvent(event);
          }
        }
        if (cancelled()) return;
        input.onEvent({ type: "message.completed" });
        input.onEvent({ type: "reasoning.completed" });
      } catch (error) {
        if (cancelled()) return;
        if (!(error instanceof EffortValidationError)) await teardown(thread);
        throw copilotError(error);
      }
    });
  thread.queue = run;
  await run;
}

export function compactCopilotContext(
  input: CompactContextInput,
): Promise<void> {
  return sendCopilotTurn({ ...input, text: "/compact" });
}

export async function steerCopilotTurn(_input: SteerTurnInput): Promise<void> {
  throw new Error("Copilot does not support steering an in-flight turn");
}

export function respondCopilotApproval(
  id: string,
  requestId: number,
  decision: ApprovalDecision,
): void {
  threads.get(id)?.live?.approvals.get(requestId)?.(decision);
}

export async function cancelCopilotTurn(id: string): Promise<void> {
  const thread = threads.get(id);
  if (!thread) return;
  thread.epoch += 1;
  const live = thread.live;
  if (live?.providerId) {
    live.muted = true;
    // Send best-effort protocol cancellation, then recycle the transport.
    void live.acp
      .notify("session/cancel", { sessionId: live.providerId })
      .catch(() => undefined);
  }
  await teardown(thread);
}

/** Parking a child retains the provider conversation for session/load. */
export async function stopCopilotSession(id: string): Promise<void> {
  const thread = threads.get(id);
  if (!thread) return;
  thread.epoch += 1;
  await teardown(thread);
}

export async function forgetCopilotSession(id: string): Promise<void> {
  const thread = threads.get(id);
  if (!thread) return;
  threads.delete(id);
  thread.epoch += 1;
  thread.resume = undefined;
  await teardown(thread);
}

export function bindCopilotSession(
  id: string,
  providerId: string,
  cwd: string,
): void {
  threadFor(id).resume = { providerId, cwd };
}

export function copilotCommands(id?: string): NativeCommand[] {
  return id ? (threads.get(id)?.commands ?? []) : [];
}

export function subscribeCopilotCommands(
  id: string,
  listener: (commands: NativeCommand[]) => void,
): () => void {
  const thread = threadFor(id);
  thread.listeners.add(listener);
  return () => thread.listeners.delete(listener);
}

async function teardown(thread: Thread): Promise<void> {
  const live = thread.live;
  if (!live) return;
  thread.live = undefined;
  live.muted = true;
  for (const resolve of live.approvals.values()) resolve("deny");
  live.approvals.clear();
  live.acp.close();
  unwatchChild(live.childKey);
  await killChild(live.childKey).catch(() => undefined);
}

async function ensureLive(
  thread: Thread,
  input: SendTurnInput,
  cancelled: () => boolean,
): Promise<Live | undefined> {
  const existing = thread.live;
  if (existing && existing.cwd === input.cwd) return existing;
  await teardown(thread);
  if (cancelled()) return;
  const { path } = await resolveCopilotBinary();
  if (cancelled()) return;
  const childKey = `${input.sessionId}:copilot:${++childSequence}`;
  const live: Live = {
    childKey,
    acp: new JsonRpcClient(
      childKey,
      {
        onNotification: (method, params) => {
          if (method !== "session/update") return;
          updateConfiguration(live, (params as { update?: unknown })?.update);
          const commands = commandsFromCopilotUpdate(params);
          if (commands) {
            thread.commands = commands;
            for (const listener of thread.listeners) listener(commands);
          }
          if (live.muted) return;
          for (const event of live.updates.route(params))
            live.input.onEvent(event);
        },
        onRequest: (id, method, params) => {
          void handleRequest(live, id, method, params).catch(() => undefined);
        },
      },
      { label: "copilot-acp" },
    ),
    providerId: "",
    cwd: input.cwd,
    effortOptions: [],
    muted: true,
    input,
    updates: new CopilotEvents(),
    approvals: new Map(),
  };
  thread.live = live;
  watchChild(
    childKey,
    (line) => live.acp.pushLine(line),
    (code) => {
      const emit = !live.muted;
      live.muted = true;
      live.acp.close(new Error("Copilot CLI exited"));
      unwatchChild(childKey);
      for (const resolve of live.approvals.values()) resolve("deny");
      live.approvals.clear();
      if (thread.live === live) thread.live = undefined;
      if (emit) live.input.onEvent({ type: "session.ended", code });
    },
  );
  try {
    await spawnChild(
      childKey,
      path,
      copilotSpawnArgs(),
      input.cwd,
      undefined,
      "copilot",
    );
    if (cancelled()) {
      // A cancel may arrive before spawnChild has registered its pid.
      await killChild(childKey).catch(() => undefined);
      return;
    }
    const initialized = await live.acp.request<{
      agentCapabilities?: { loadSession?: boolean };
    }>("initialize", COPILOT_INITIALIZE_PARAMS, 20_000);
    const resume = thread.resume?.cwd === input.cwd ? thread.resume : undefined;
    if (resume) {
      if (!initialized.agentCapabilities?.loadSession)
        throw new Error(
          "Copilot CLI cannot resume this session. Update the CLI and retry.",
        );
      // Never silently start an empty conversation when a resume fails.
      const setup = await live.acp.request(
        "session/load",
        { sessionId: resume.providerId, cwd: input.cwd, mcpServers: [] },
        45_000,
      );
      updateConfiguration(live, setup);
      live.providerId = resume.providerId;
    } else {
      const setup = await live.acp.request(
        "session/new",
        { cwd: input.cwd, mcpServers: [] },
        45_000,
      );
      updateConfiguration(live, setup);
      live.providerId = sessionIdFromResult(setup) ?? "";
    }
    if (cancelled()) return;
    if (!live.providerId)
      throw new Error("Copilot CLI did not return a session id");
    thread.resume = { providerId: live.providerId, cwd: input.cwd };
    input.onEvent({
      type: "session.providerBound",
      providerSessionId: live.providerId,
    });
    input.onEvent({ type: "session.started" });
    return live;
  } catch (error) {
    if (thread.live === live) await teardown(thread);
    throw error;
  }
}

function resetEffort(live: Live): void {
  live.effort = undefined;
  live.defaultEffort = undefined;
  live.effortOptions = [];
}

function updateConfiguration(live: Live, result: unknown): void {
  const options = copilotConfigOptions(result);
  const model = options.find((option) => option.id === "model");
  const effort = options.find((option) => option.id === "reasoning_effort");
  // Reset before ingesting effort: model notifications can precede set_model's
  // response, so comparing against the old modelId must happen first.
  if (
    (typeof model?.currentValue === "string" &&
      model.currentValue !== live.modelId) ||
    (options.length > 0 && !effort)
  )
    resetEffort(live);
  for (const option of options) {
    if (typeof option.currentValue !== "string") continue;
    if (option.id === "mode") live.modeId = copilotModeId(option.currentValue);
    if (option.id === "model") live.modelId = option.currentValue;
  }
  if (typeof effort?.currentValue === "string") {
    live.effort = effort.currentValue;
    live.defaultEffort ??= effort.currentValue;
    live.effortOptions =
      effort.options?.flatMap((entry) =>
        typeof entry.value === "string" ? [entry.value] : [],
      ) ?? [];
  }
}

async function handleRequest(
  live: Live,
  wireId: JsonRpcId,
  method: string,
  params: unknown,
): Promise<void> {
  if (method !== "session/request_permission") {
    await live.acp.respondError(wireId, {
      code: -32601,
      message: `Method not found: ${method}`,
    });
    return;
  }
  const request = permissionRequestFromAcp(params);
  const kind = request.kind;
  let allow: boolean;
  if (live.muted) allow = false;
  else if (live.input.intent === "plan")
    allow = kind === "read" || kind === "search";
  else if (live.input.runtimeMode === "full-access") allow = true;
  else if (
    live.input.runtimeMode === "auto-accept-edits" &&
    ["read", "search", "edit"].includes(kind ?? "")
  )
    allow = true;
  else {
    const requestId = ++approvalSequence;
    // Register before emitting; a synchronous UI/host reply must not be lost.
    const pending = new Promise<ApprovalDecision>((resolve) =>
      live.approvals.set(requestId, resolve),
    );
    live.input.onEvent({
      type: "approval.requested",
      requestId,
      title: request.title,
      kind,
      callId: request.callId,
      preview: request.preview,
    });
    const decision = await pending;
    live.approvals.delete(requestId);
    live.input.onEvent({ type: "approval.resolved", requestId, decision });
    allow = decision === "allow" && !live.muted;
  }
  const optionId = copilotPermissionOption(params, allow);
  await live.acp.respond(wireId, {
    outcome: optionId
      ? { outcome: "selected", optionId }
      : { outcome: "cancelled" },
  });
}

import { modelsFor } from "../../../../features/sessions/model/models";
import {
  killChild,
  resolveClaudeBinary,
  spawnChild,
  unwatchChild,
  watchChild,
  writeChild,
} from "../../core/child";
import { isAgentToolName } from "../../core/preview";
import {
  assistantTextBlocks,
  toolResultsFromUserMessage,
  buildClaudeSpawnArgs,
  buildClaudeUserMessage,
  buildControlRequest,
  parseControlResponse,
  inputJsonDeltaFromEvent,
  isClaudeUltracodeEffort,
  normalizeClaudeCliEffort,
  parseJsonLine,
  previewFromTool,
  resolveClaudeApiModelId,
  streamDeltaFromEvent,
  stringField,
  summarizeToolRequest,
  toolKindFromName,
  toolStartFromEvent,
  toolTitle,
  tryParseJsonRecord,
  turnStatusFromResult,
} from "./claudeProtocol";
import type { TurnIntent } from "../../../../features/sessions/model/session";
import type { HarnessEvent } from "../../core/types";
import { snapshotRemainder } from "../../core/streamText";

const TEXT_CHILD_ID = "monocode-claude-text";
const INIT_TIMEOUT_MS = 8_000;
const REQUEST_TIMEOUT_MS = 45_000;
const TEXT_MODEL = "claude-haiku-4-5";

type TextSettings = {
  key: string;
  launchModel: string;
  effort?: string;
  promptEffort?: string;
  settings: Record<string, boolean>;
  permissionMode?: "plan";
  maxTurns?: number;
  tools: string[];
};

type InFlightTool = {
  id: string;
  name: string;
  input: Record<string, unknown>;
  partialJson: string;
  title: string;
};

type LiveText = {
  cwd: string;
  providerAccountId?: string;
  model: string;
  settingsKey: string;
  collecting: boolean;
  output: string;
  messageText: string;
  closed: boolean;
  ready: boolean;
  initError: Error | null;
  readyFailed: ((error: Error) => void) | null;
  turnDone: (() => void) | null;
  turnFailed: ((error: Error) => void) | null;
  readyDone: (() => void) | null;
  onEvent?: (event: HarnessEvent) => void;
  toolsByIndex: Map<number, InFlightTool>;
  toolsById: Map<string, InFlightTool>;
};

let live: LiveText | null = null;
let turns: Promise<void> = Promise.resolve();

function textSettings(
  model: string,
  modelSettings?: Record<string, string>,
  intent?: TurnIntent,
): TextSettings {
  const effort = modelSettings?.effort?.trim() || undefined;
  const context = modelSettings?.context?.trim() || undefined;
  const thinking = modelSettings?.thinking === "true";
  const fast = modelSettings?.fast === "true";
  const readOnly = intent === "plan";
  const settings: Record<string, boolean> = {};
  if (modelSettings?.thinking !== undefined)
    settings.alwaysThinkingEnabled = thinking;
  if (fast) settings.fastMode = true;
  if (isClaudeUltracodeEffort(effort)) settings.ultracode = true;
  return {
    key: JSON.stringify({ effort, context, settings, readOnly }),
    launchModel: resolveClaudeApiModelId(model, context),
    effort: normalizeClaudeCliEffort(effort, model),
    promptEffort: effort,
    settings,
    tools: readOnly ? ["Read", "Glob", "Grep"] : [],
    maxTurns: readOnly ? 12 : 1,
    ...(readOnly ? { permissionMode: "plan" as const } : {}),
  };
}

function pickTextModel(requested?: string): string {
  const selected = requested?.trim();
  if (selected) return selected;
  const models = modelsFor("claude");
  const haiku = models.find((model) =>
    /haiku/i.test(`${model.nativeId ?? ""} ${model.name} ${model.id}`),
  );
  return haiku?.nativeId ?? TEXT_MODEL;
}

export async function stopClaudeTextPrompt(): Promise<void> {
  await dropLive();
}

export function warmupClaudeText(cwd: string): Promise<void> {
  if (!cwd || cwd === "~") return Promise.resolve();
  const run = turns
    .catch(() => undefined)
    .then(async () => {
      await ensureLive(cwd);
    });
  turns = run.then(
    () => undefined,
    () => undefined,
  );
  return run.catch(() => undefined);
}

export async function runClaudeTextPrompt(input: {
  cwd: string;
  providerAccountId?: string;
  model?: string;
  modelSettings?: Record<string, string>;
  intent?: TurnIntent;
  prompt: string;
  timeoutMs?: number;
  signal?: AbortSignal;
  onEvent?: (event: HarnessEvent) => void;
}): Promise<string> {
  const run = turns.catch(() => undefined).then(() => promptOnLive(input));
  turns = run.then(
    () => undefined,
    () => undefined,
  );
  return run;
}

async function promptOnLive(input: {
  cwd: string;
  providerAccountId?: string;
  model?: string;
  modelSettings?: Record<string, string>;
  intent?: TurnIntent;
  prompt: string;
  timeoutMs?: number;
  signal?: AbortSignal;
  onEvent?: (event: HarnessEvent) => void;
}): Promise<string> {
  input.signal?.throwIfAborted();
  const model = pickTextModel(input.model);
  const settings = textSettings(model, input.modelSettings, input.intent);
  const session = await ensureLive(
    input.cwd,
    input.providerAccountId,
    model,
    settings,
  );
  input.signal?.throwIfAborted();
  session.output = "";
  session.messageText = "";
  session.collecting = true;
  session.onEvent = input.onEvent;
  session.toolsByIndex = new Map();
  session.toolsById = new Map();
  const timeoutMs = input.timeoutMs ?? REQUEST_TIMEOUT_MS;
  let abortHandler: (() => void) | undefined;
  let requestTimer: ReturnType<typeof setTimeout> | undefined;
  const abortPromise = input.signal
    ? new Promise<never>((_, reject) => {
        const cancel = () => {
          session.turnFailed?.(new Error("By-the-way request cancelled"));
          reject(new Error("By-the-way request cancelled"));
        };
        abortHandler = cancel;
        input.signal!.addEventListener("abort", cancel, { once: true });
        if (input.signal!.aborted) cancel();
      })
    : null;

  try {
    const turnPromise = new Promise<void>((resolve, reject) => {
      session.turnDone = resolve;
      session.turnFailed = reject;
    });

    await writeChild(
      TEXT_CHILD_ID,
      JSON.stringify(
        buildClaudeUserMessage({
          text: input.prompt,
          effort: settings.promptEffort,
        }),
      ),
    );

    await Promise.race([
      turnPromise,
      new Promise<void>((_, reject) => {
        requestTimer = setTimeout(
          () => reject(new Error("Claude text generation timed out")),
          timeoutMs,
        );
      }),
      ...(abortPromise ? [abortPromise] : []),
    ]);

    const output = session.output.trim();
    if (!output) throw new Error("Claude returned empty output.");
    return output;
  } catch (error) {
    await dropLive();
    throw error;
  } finally {
    if (requestTimer) clearTimeout(requestTimer);
    if (abortHandler && input.signal) {
      input.signal.removeEventListener("abort", abortHandler);
    }
    session.collecting = false;
    session.turnDone = null;
    session.turnFailed = null;
    await dropLive();
  }
}

async function ensureLive(
  cwd: string,
  providerAccountId?: string,
  requestedModel?: string,
  requestedSettings?: TextSettings,
): Promise<LiveText> {
  const model = pickTextModel(requestedModel);
  const settings = requestedSettings ?? textSettings(model);
  if (
    live &&
    !live.closed &&
    live.cwd === cwd &&
    live.providerAccountId === providerAccountId &&
    live.model === model &&
    live.settingsKey === settings.key
  ) {
    return live;
  }
  await dropLive();
  return startLive(cwd, providerAccountId, model, settings);
}

async function startLive(
  cwd: string,
  providerAccountId?: string,
  model = pickTextModel(),
  settings = textSettings(model),
): Promise<LiveText> {
  const { path } = await resolveClaudeBinary();
  const session: LiveText = {
    cwd,
    providerAccountId,
    model,
    settingsKey: settings.key,
    collecting: false,
    output: "",
    messageText: "",
    closed: false,
    ready: false,
    initError: null,
    readyFailed: null,
    turnDone: null,
    turnFailed: null,
    readyDone: null,
    toolsByIndex: new Map(),
    toolsById: new Map(),
  };

  watchChild(
    TEXT_CHILD_ID,
    (line) => handleLine(session, line),
    () => {
      session.closed = true;
      if (live === session) live = null;
      session.turnFailed?.(new Error("Claude text generator exited"));
      session.readyDone?.();
      session.turnDone = null;
      session.turnFailed = null;
      session.readyDone = null;
    },
  );

  try {
    await spawnChild(
      TEXT_CHILD_ID,
      path,
      buildClaudeSpawnArgs({
        isolated: true,
        model: settings.launchModel,
        effort: settings.effort,
        settings: settings.settings,
        permissionMode: settings.permissionMode,
        maxTurns: settings.maxTurns,
        tools: settings.tools,
      }),
      cwd,
      { provider: "claude", id: providerAccountId ?? "default" },
      "claude",
    );
    live = session;
    await writeChild(
      TEXT_CHILD_ID,
      JSON.stringify(
        buildControlRequest("monocode_text_init", { subtype: "initialize" }),
      ),
    );
    await waitForReady(session, INIT_TIMEOUT_MS);
    if (session.closed)
      throw new Error("Claude text generator exited during initialization");
    return session;
  } catch (error) {
    session.closed = true;
    unwatchChild(TEXT_CHILD_ID);
    await killChild(TEXT_CHILD_ID).catch(() => undefined);
    throw error;
  }
}

async function dropLive(): Promise<void> {
  const current = live;
  live = null;
  if (current) {
    current.closed = true;
    current.readyDone?.();
    current.turnFailed?.(new Error("Claude text generator stopped"));
    current.turnDone = null;
    current.turnFailed = null;
    current.readyDone = null;
  }
  unwatchChild(TEXT_CHILD_ID);
  await killChild(TEXT_CHILD_ID).catch(() => undefined);
}

function handleLine(session: LiveText, line: string): void {
  const rec = parseJsonLine(line);
  if (!rec) return;
  const type = stringField(rec, "type");
  const control = parseControlResponse(rec);
  if (control?.requestId === "monocode_text_init") {
    if (control.ok) {
      session.ready = true;
      session.readyDone?.();
    } else {
      session.initError = new Error(
        control.error ?? "Claude text generator initialization failed",
      );
      session.readyFailed?.(session.initError);
    }
    return;
  }
  if (!session.collecting) return;
  if (type === "assistant") {
    const snapshot = assistantTextBlocks(rec).join("");
    const extra = snapshotRemainder(session.messageText, snapshot);
    session.output += extra;
    if (extra)
      session.onEvent?.({ type: "message.delta", text: extra, append: true });
    session.messageText = "";
    session.onEvent?.({ type: "message.completed" });
    return;
  }
  if (type === "user") {
    for (const result of toolResultsFromUserMessage(rec)) {
      const tool = session.toolsById.get(result.toolUseId);
      if (!tool) continue;
      session.onEvent?.({
        type: "tool.updated",
        callId: tool.id,
        status: result.isError ? "failed" : "completed",
        detail: result.text.slice(0, 32_000),
        preview: previewFromTool(tool.name, tool.input, result.text),
      });
    }
    return;
  }
  if (type === "stream_event") {
    handleStreamEvent(session, rec);
    return;
  }
  if (type === "result") {
    const result = turnStatusFromResult(rec);
    if (result.status === "failed") {
      session.turnFailed?.(new Error(result.error ?? "Claude turn failed"));
    } else {
      session.turnDone?.();
    }
    session.turnDone = null;
    session.turnFailed = null;
  }
}

function handleStreamEvent(
  session: LiveText,
  rec: Record<string, unknown>,
): void {
  const delta = streamDeltaFromEvent(rec);
  if (delta) {
    if (delta.kind === "assistant") {
      session.output += delta.text;
      session.messageText += delta.text;
      session.onEvent?.({
        type: "message.delta",
        text: delta.text,
        append: true,
      });
    } else {
      session.onEvent?.({
        type: "reasoning.delta",
        text: delta.text,
        append: true,
      });
    }
    return;
  }

  const started = toolStartFromEvent(rec);
  if (started) {
    const tool: InFlightTool = {
      id: started.id,
      name: started.name,
      input: started.input,
      partialJson: "",
      title: toolTitle(started.name, started.input),
    };
    if (started.index >= 0) session.toolsByIndex.set(started.index, tool);
    session.toolsById.set(started.id, tool);
    session.onEvent?.({
      type: "tool.started",
      callId: tool.id,
      title: tool.title,
      kind: toolKindFromName(tool.name),
      ...(isAgentToolName(tool.name) && stringField(tool.input, "model")
        ? { agentModel: stringField(tool.input, "model") }
        : {}),
      status: isAgentToolName(tool.name) ? "in_progress" : "pending",
      preview: previewFromTool(tool.name, tool.input),
    });
    return;
  }

  const jsonDelta = inputJsonDeltaFromEvent(rec);
  if (!jsonDelta) return;
  const tool = session.toolsByIndex.get(jsonDelta.index);
  if (!tool) return;
  tool.partialJson += jsonDelta.partial;
  const parsed = tryParseJsonRecord(tool.partialJson);
  if (!parsed) return;
  tool.input = parsed;
  tool.title = toolTitle(tool.name, parsed);
  session.onEvent?.({
    type: "tool.updated",
    callId: tool.id,
    title: tool.title,
    kind: toolKindFromName(tool.name),
    ...(isAgentToolName(tool.name) && stringField(tool.input, "model")
      ? { agentModel: stringField(tool.input, "model") }
      : {}),
    status: "pending",
    detail: summarizeToolRequest(tool.name, parsed),
    preview: previewFromTool(tool.name, parsed),
  });
}

function waitForReady(session: LiveText, timeoutMs: number): Promise<void> {
  if (session.initError) return Promise.reject(session.initError);
  if (session.closed)
    return Promise.reject(new Error("Claude text generator exited"));
  if (session.ready) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      session.readyDone = null;
      reject(new Error("Claude text generator initialization timed out"));
    }, timeoutMs);
    session.readyFailed = (error) => {
      clearTimeout(timer);
      session.readyDone = null;
      session.readyFailed = null;
      reject(error);
    };
    session.readyDone = () => {
      session.readyFailed = null;
      clearTimeout(timer);
      if (session.closed) {
        reject(new Error("Claude text generator exited"));
        return;
      }
      resolve();
    };
  });
}

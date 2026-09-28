import { modelsFor } from "../../../../features/sessions/model/models";
import {
  killChild,
  resolveCodexBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import {
  asRecord,
  buildThreadStartParams,
  buildTurnStartParams,
  isRecoverableThreadResumeError,
  mapCodexNotification,
  stringField,
} from "./codexProtocol";
import type { HarnessEvent } from "../../core/types";
import { JsonRpcClient, type JsonRpcId } from "../../core/jsonRpc";
import type { TurnIntent } from "../../../../features/sessions/model/session";

import { mergeStream, streamTextDelta } from "../../core/streamText";

const TEXT_CHILD_ID = "monocode-codex-text";
const INIT_TIMEOUT_MS = 60_000;
const REQUEST_TIMEOUT_MS = 45_000;
const TEXT_RUNTIME_MODE = "supervised" as const;
const TEXT_MODEL = "gpt-5.6-luna";
const TEXT_EFFORT = "low";

type LiveText = {
  rpc: JsonRpcClient;
  cwd: string;
  providerAccountId?: string;
  threadId: string;
  model: string;
  effort: string;
  serviceTier?: string;
  collecting: boolean;
  output: string;
  closed: boolean;
  turnDone: (() => void) | null;
  turnFailed: ((error: Error) => void) | null;
  onEvent?: (event: HarnessEvent) => void;
};

let live: LiveText | null = null;
let turns: Promise<void> = Promise.resolve();

function pickTextModel(requested?: string): string {
  const selected = requested?.trim();
  if (selected) return selected;
  const models = modelsFor("codex");
  const luna = models.find((model) =>
    /5\.6-luna/i.test(`${model.nativeId ?? ""} ${model.name} ${model.id}`),
  );
  return luna?.nativeId ?? TEXT_MODEL;
}

function pickTextEffort(
  modelId: string,
  modelSettings?: Record<string, string>,
): string {
  const requested =
    modelSettings?.reasoningEffort?.trim() || modelSettings?.effort?.trim();
  const model = modelsFor("codex").find((entry) => entry.nativeId === modelId);
  const setting = model?.settings?.find(
    (entry) => entry.id === "reasoningEffort",
  );
  const options = setting?.options?.map((option) => option.value) ?? [];
  if (requested && (options.length === 0 || options.includes(requested))) {
    return requested;
  }
  if (options.includes("low")) return "low";
  if (options.includes("none")) return "none";
  if (setting?.value && options.includes(setting.value)) return setting.value;
  return TEXT_EFFORT;
}

function pickTextServiceTier(
  modelSettings?: Record<string, string>,
): string | undefined {
  const value = modelSettings?.serviceTier?.trim();
  return value || undefined;
}

export async function stopCodexTextPrompt(): Promise<void> {
  await dropLive();
}

/** Start the shared Codex app-server in the background so the first prompt is fast. */
export function warmupCodexText(cwd: string): Promise<void> {
  if (!cwd || cwd === "~") return Promise.resolve();
  const run = turns
    .catch(() => undefined)
    .then(async () => {
      await ensureLive({ cwd });
    });
  turns = run.then(
    () => undefined,
    () => undefined,
  );
  return run.catch(() => undefined);
}

/** Codex app-server turn that reuses a warm process, like Cursor text generation. */
export async function runCodexTextPrompt(input: {
  cwd: string;
  providerAccountId?: string;
  model?: string;
  modelSettings?: Record<string, string>;
  threadId?: string;
  onThreadId?: (threadId: string) => void;
  intent?: TurnIntent;
  prompt: string;
  timeoutMs?: number;
  signal?: AbortSignal;
  onEvent?: (event: HarnessEvent) => void;
}): Promise<string> {
  if (input.model !== undefined && !input.model.trim()) {
    throw new Error("The selected Codex model is unavailable.");
  }
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
  threadId?: string;
  onThreadId?: (threadId: string) => void;
  intent?: TurnIntent;
  prompt: string;
  timeoutMs?: number;
  signal?: AbortSignal;
  onEvent?: (event: HarnessEvent) => void;
}): Promise<string> {
  const session = await ensureLive(input);
  session.output = "";
  session.collecting = true;
  session.onEvent = input.onEvent;
  const timeoutMs = input.timeoutMs ?? REQUEST_TIMEOUT_MS;
  let abortHandler: (() => void) | undefined;
  const abortPromise = input.signal
    ? new Promise<void>((_, reject) => {
        abortHandler = () => {
          void session.rpc
            .request("turn/interrupt", { threadId: session.threadId })
            .catch(() => undefined);
          reject(new Error("By-the-way request cancelled"));
        };
        input.signal!.addEventListener("abort", abortHandler, { once: true });
        if (input.signal!.aborted) abortHandler();
      })
    : null;

  try {
    const turnPromise = new Promise<void>((resolve, reject) => {
      session.turnDone = resolve;
      session.turnFailed = reject;
    });

    const turnStart = session.rpc.request(
      "turn/start",
      buildTurnStartParams({
        threadId: session.threadId,
        runtimeMode: TEXT_RUNTIME_MODE,
        prompt: input.prompt,
        model: session.model || undefined,
        effort: session.effort,
        serviceTier: session.serviceTier,
        intent: input.intent,
      }),
      timeoutMs,
    );
    await Promise.race([turnStart, ...(abortPromise ? [abortPromise] : [])]);

    await Promise.race([
      turnPromise,
      new Promise<void>((_, reject) => {
        setTimeout(
          () => reject(new Error("Codex text generation timed out")),
          timeoutMs,
        );
      }),
      ...(abortPromise ? [abortPromise] : []),
    ]);

    return session.output;
  } catch (error) {
    await session.rpc
      .request("turn/interrupt", { threadId: session.threadId })
      .catch(() => undefined);
    if (session.closed) await dropLive();
    throw error;
  } finally {
    if (abortHandler && input.signal) {
      input.signal.removeEventListener("abort", abortHandler);
    }
    session.collecting = false;
    session.turnDone = null;
    session.turnFailed = null;
    await dropLive();
  }
}
async function ensureLive(input: {
  cwd: string;
  providerAccountId?: string;
  model?: string;
  modelSettings?: Record<string, string>;
  threadId?: string;
  onThreadId?: (threadId: string) => void;
}): Promise<LiveText> {
  const model = pickTextModel(input.model);
  const effort = pickTextEffort(model, input.modelSettings);
  const serviceTier = pickTextServiceTier(input.modelSettings);
  const requestedThreadId = input.threadId?.trim() || undefined;
  if (live && !live.closed) {
    if (
      live.cwd === input.cwd &&
      live.model === model &&
      live.effort === effort &&
      live.serviceTier === serviceTier &&
      live.providerAccountId === input.providerAccountId &&
      (!requestedThreadId || live.threadId === requestedThreadId)
    ) {
      input.onThreadId?.(live.threadId);
      return live;
    }
    if (live.providerAccountId !== input.providerAccountId) {
      await dropLive();
      const started = await startLive(
        input.cwd,
        input.providerAccountId,
        model,
        effort,
        serviceTier,
        requestedThreadId,
      );
      input.onThreadId?.(started.threadId);
      return started;
    }
    try {
      live.model = model;
      live.effort = effort;
      live.serviceTier = serviceTier;
      await openThread(live, input.cwd, requestedThreadId);
      input.onThreadId?.(live.threadId);
      return live;
    } catch (error) {
      await dropLive();
      throw error;
    }
  }
  const started = await startLive(
    input.cwd,
    input.providerAccountId,
    model,
    effort,
    serviceTier,
    requestedThreadId,
  );
  input.onThreadId?.(started.threadId);
  return started;
}

async function startLive(
  cwd: string,
  providerAccountId?: string,
  model = pickTextModel(),
  effort = pickTextEffort(model),
  serviceTier?: string,
  requestedThreadId?: string,
): Promise<LiveText> {
  await dropLive();
  const { path } = await resolveCodexBinary();
  const sessionRef: { session: LiveText | null } = { session: null };
  const rpc = new JsonRpcClient(
    TEXT_CHILD_ID,
    {
      onNotification: (method, params) => {
        handleNotification(sessionRef.session, method, params);
      },
      onRequest: (id, method, params) => {
        void handleServerRequest(rpc, id, method, params);
      },
    },
    { includeJsonrpc: false, label: "codex-text" },
  );

  const session: LiveText = {
    rpc,
    cwd,
    providerAccountId,
    threadId: "",
    model,
    effort,
    serviceTier,
    collecting: false,
    output: "",
    closed: false,
    turnDone: null,
    turnFailed: null,
  };

  sessionRef.session = session;

  watchChild(
    TEXT_CHILD_ID,
    (line) => rpc.pushLine(line),
    () => {
      session.closed = true;
      if (live === session) live = null;
      session.turnFailed?.(new Error("Codex text generator exited"));
      session.turnDone = null;
      session.turnFailed = null;
      rpc.close(new Error("Codex text generator exited"));
    },
  );

  try {
    await spawnChild(
      TEXT_CHILD_ID,
      path,
      ["app-server"],
      cwd,
      {
        provider: "codex",
        id: providerAccountId ?? "default",
      },
      "codex",
    );
    await rpc.request(
      "initialize",
      {
        clientInfo: {
          name: "monocode-text",
          title: "MonoCode",
          version: "0.1.0",
        },
        capabilities: { experimentalApi: true },
      },
      INIT_TIMEOUT_MS,
    );
    await rpc.notify("initialized", undefined);
    await openThread(session, cwd, requestedThreadId);
    live = session;
    return session;
  } catch (error) {
    session.closed = true;
    rpc.close(error instanceof Error ? error : new Error(String(error)));
    unwatchChild(TEXT_CHILD_ID);
    await killChild(TEXT_CHILD_ID).catch(() => undefined);
    throw error;
  }
}

async function openThread(
  session: LiveText,
  cwd: string,
  requestedThreadId?: string,
): Promise<void> {
  let opened: { thread?: { id?: string } } | undefined;
  if (requestedThreadId) {
    try {
      opened = await session.rpc.request<{ thread?: { id?: string } }>(
        "thread/resume",
        {
          threadId: requestedThreadId,
          ...buildThreadStartParams({
            cwd,
            runtimeMode: TEXT_RUNTIME_MODE,
            model: session.model || undefined,
            serviceTier: session.serviceTier,
          }),
        },
        INIT_TIMEOUT_MS,
      );
    } catch (error) {
      if (!isRecoverableThreadResumeError(error)) throw error;
      opened = undefined;
    }
    if (!opened?.thread?.id?.trim()) opened = undefined;
  }
  if (!opened) {
    opened = await session.rpc.request<{ thread?: { id?: string } }>(
      "thread/start",
      buildThreadStartParams({
        cwd,
        runtimeMode: TEXT_RUNTIME_MODE,
        model: session.model || undefined,
        serviceTier: session.serviceTier,
      }),
      INIT_TIMEOUT_MS,
    );
  }
  const threadId = opened.thread?.id?.trim();
  if (!threadId) throw new Error("Codex did not return a thread id");
  session.cwd = cwd;
  session.threadId = threadId;
}
async function dropLive(): Promise<void> {
  const current = live;
  live = null;
  if (current) {
    current.closed = true;
    current.rpc.close();
  }
  unwatchChild(TEXT_CHILD_ID);
  await killChild(TEXT_CHILD_ID).catch(() => undefined);
}

function handleNotification(
  session: LiveText | null,
  method: string,
  params: unknown,
): void {
  if (!session || !session.collecting) {
    if (session && method === "turn/completed") {
      session.turnDone?.();
      session.turnDone = null;
      session.turnFailed = null;
    }
    return;
  }

  const mapped = mapCodexNotification(method, params);
  for (const event of mapped.events) {
    session.onEvent?.(event);
  }

  if (method === "item/agentMessage/delta") {
    const delta = streamTextDelta(asRecord(params)?.delta);
    if (delta) session.output = mergeStream(session.output, delta);
    return;
  }

  if (method === "turn/completed") {
    const turn = asRecord(asRecord(params)?.turn);
    const status = stringField(turn, "status") ?? "completed";
    if (status === "failed") {
      const message =
        stringField(asRecord(turn?.error), "message") ?? "Codex turn failed";
      session.turnFailed?.(new Error(message));
    } else {
      session.turnDone?.();
    }
    session.turnDone = null;
    session.turnFailed = null;
  }
}

async function handleServerRequest(
  rpc: JsonRpcClient,
  id: JsonRpcId,
  method: string,
  _params: unknown,
): Promise<void> {
  if (
    method === "item/commandExecution/requestApproval" ||
    method === "item/fileChange/requestApproval"
  ) {
    await rpc.respond(id, { decision: "decline" }).catch(() => undefined);
    return;
  }
  if (method === "item/permissions/requestApproval") {
    await rpc.respond(id, { permissions: {} }).catch(() => undefined);
    return;
  }
  await rpc.respond(id, {}).catch(() => undefined);
}

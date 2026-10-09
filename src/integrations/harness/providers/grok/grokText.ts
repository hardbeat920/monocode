import { AcpClient } from "../../core/acp";
import {
  execChild,
  killChild,
  resolveGrokBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import {
  grokAuthMethodId,
  grokEffort,
  grokTextSpawnArgs,
  TEXT_MODEL,
} from "./grokProtocol";
import { abortTextPromptRace } from "../../core/abortTextPrompt";
import { mergeStream } from "../../core/streamText";
import type { HarnessEvent } from "../../core/types";

const TEXT_CHILD_ID = "monocode-grok-text";
const INIT_TIMEOUT_MS = 60_000;
const REQUEST_TIMEOUT_MS = 20_000;

const CLIENT_CAPABILITIES = {
  fs: { readTextFile: false, writeTextFile: false },
  terminal: false,
};

type LiveText = {
  acp: AcpClient;
  binaryPath: string;
  cwd: string;
  model: string;
  settingsKey: string;
  acpSessionId: string;
  collecting: boolean;
  output: string;
  closed: boolean;
  onEvent?: (event: HarnessEvent) => void;
};

let live: LiveText | null = null;
let turns: Promise<void> = Promise.resolve();

export async function stopGrokTextPrompt(childId?: string): Promise<void> {
  await dropLive();
  if (childId && childId !== TEXT_CHILD_ID) {
    unwatchChild(childId);
    await killChild(childId).catch(() => undefined);
  }
}

export function warmupGrokText(cwd: string): Promise<void> {
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

export async function runGrokTextPrompt(input: {
  cwd: string;
  model?: string;
  modelSettings?: Record<string, string>;
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
  model?: string;
  modelSettings?: Record<string, string>;
  prompt: string;
  timeoutMs?: number;
  signal?: AbortSignal;
  onEvent?: (event: HarnessEvent) => void;
}): Promise<string> {
  input.signal?.throwIfAborted();
  const session = await ensureLive(input.cwd, input.model, input.modelSettings);
  input.signal?.throwIfAborted();
  session.output = "";
  session.collecting = true;
  session.onEvent = input.onEvent;
  const abort = abortTextPromptRace(input.signal, () =>
    session.acp.notify("session/cancel", {
      sessionId: session.acpSessionId,
    }),
  );
  try {
    await Promise.race([
      session.acp.request(
        "session/prompt",
        {
          sessionId: session.acpSessionId,
          prompt: [{ type: "text", text: input.prompt }],
        },
        input.timeoutMs ?? REQUEST_TIMEOUT_MS,
      ),
      ...(abort.promise ? [abort.promise] : []),
    ]);
    return session.output;
  } catch (error) {
    await session.acp
      .notify("session/cancel", { sessionId: session.acpSessionId })
      .catch(() => undefined);
    if (session.closed) await dropLive();
    throw error;
  } finally {
    abort.detach();
    session.collecting = false;
    session.onEvent = undefined;
    await dropLive();
  }
}

async function ensureLive(
  cwd: string,
  requestedModel?: string,
  modelSettings?: Record<string, string>,
): Promise<LiveText> {
  const model = requestedModel?.trim() || TEXT_MODEL;
  const settingsKey = modelSettingsKey(modelSettings);
  if (
    live &&
    !live.closed &&
    live.cwd === cwd &&
    live.model === model &&
    live.settingsKey === settingsKey
  )
    return live;
  // Retire the previous throwaway session before replacing its id, including
  // when a warmed-up process needs a different model or working directory.
  return startLive(cwd, model, modelSettings);
}

async function startLive(
  cwd: string,
  model = TEXT_MODEL,
  modelSettings?: Record<string, string>,
): Promise<LiveText> {
  await dropLive();
  const { path } = await resolveGrokBinary();
  const acpRef: { session: LiveText | null } = { session: null };
  const acp = new AcpClient(TEXT_CHILD_ID, {
    onNotification: (method, params) => {
      const session = acpRef.session;
      if (!session || method !== "session/update" || !session.collecting)
        return;
      const previous = session.output;
      session.output = mergeStream(previous, textFromUpdate(params));
      const delta = session.output.slice(previous.length);
      if (delta) session.onEvent?.({ type: "message.delta", text: delta });
    },
    onRequest: (id, method, params) => {
      void handleTextRequest(acp, id, method, params);
    },
  });
  const session: LiveText = {
    acp,
    binaryPath: path,
    cwd,
    model,
    settingsKey: modelSettingsKey(modelSettings),
    acpSessionId: "",
    collecting: false,
    output: "",
    closed: false,
    onEvent: undefined,
  };
  acpRef.session = session;

  watchChild(
    TEXT_CHILD_ID,
    (line) => acp.pushLine(line),
    () => {
      session.closed = true;
      // Keep the closed session available for history cleanup in dropLive.
      acp.close(new Error("Grok Build text generator exited"));
    },
  );

  try {
    await spawnChild(
      TEXT_CHILD_ID,
      path,
      grokTextSpawnArgs(),
      cwd,
      undefined,
      "grok",
    );
    const init = await acp.request(
      "initialize",
      {
        protocolVersion: 1,
        clientCapabilities: CLIENT_CAPABILITIES,
        clientInfo: { name: "monocode-text", version: "0.1.0" },
      },
      INIT_TIMEOUT_MS,
    );
    const methodId = grokAuthMethodId(init);
    if (methodId) {
      await acp
        .request(
          "authenticate",
          { methodId, _meta: { headless: true } },
          REQUEST_TIMEOUT_MS,
        )
        .catch(() => undefined);
    }
    await openSession(session, cwd, model, modelSettings);
    live = session;
    return session;
  } catch (error) {
    session.closed = true;
    acp.close(error instanceof Error ? error : new Error(String(error)));
    unwatchChild(TEXT_CHILD_ID);
    await killChild(TEXT_CHILD_ID).catch(() => undefined);
    await deleteTextSession(session);
    throw error;
  }
}

async function openSession(
  session: LiveText,
  cwd: string,
  model: string,
  modelSettings?: Record<string, string>,
): Promise<void> {
  const setup = await session.acp.request<{ sessionId?: string }>(
    "session/new",
    { cwd, mcpServers: [] },
    REQUEST_TIMEOUT_MS,
  );
  const acpSessionId = setup.sessionId?.trim();
  if (!acpSessionId) throw new Error("Grok Build did not return a session id");
  session.acpSessionId = acpSessionId;

  await session.acp
    .request(
      "session/set_model",
      { sessionId: acpSessionId, modelId: model },
      REQUEST_TIMEOUT_MS,
    )
    .catch(() => undefined);
  await session.acp
    .request(
      "session/set_mode",
      {
        sessionId: acpSessionId,
        modeId: grokEffort(modelSettings) ?? "low",
      },
      REQUEST_TIMEOUT_MS,
    )
    .catch(() => undefined);

  session.cwd = cwd;
  session.model = model;
  session.settingsKey = modelSettingsKey(modelSettings);
}

async function dropLive(): Promise<void> {
  const current = live;
  live = null;
  if (current) {
    current.closed = true;
    current.acp.close();
  }
  unwatchChild(TEXT_CHILD_ID);
  await killChild(TEXT_CHILD_ID).catch(() => undefined);
  // Stop the writer first so it cannot recreate the session after deletion.
  if (current) await deleteTextSession(current);
}

async function deleteTextSession(session: LiveText): Promise<void> {
  if (
    !/^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/i.test(session.acpSessionId)
  )
    return;
  await execChild(
    session.binaryPath,
    ["--no-auto-update", "sessions", "delete", session.acpSessionId],
    session.cwd,
    "grok",
  ).catch((error) =>
    console.debug("[monocode] Grok text session cleanup", error),
  );
}

async function handleTextRequest(
  acp: AcpClient,
  id: number,
  method: string,
  params: unknown,
) {
  if (method === "session/request_permission") {
    const optionIds = permissionOptionIds(params);
    const optionId =
      optionIds.find((value) => /reject|deny|cancel/i.test(value)) ??
      "reject-once";
    await acp
      .respond(id, { outcome: { outcome: "selected", optionId } })
      .catch(() => undefined);
    return;
  }
  if (
    method === "_x.ai/ask_user_question" ||
    method === "x.ai/ask_user_question"
  ) {
    await acp.respond(id, { outcome: "skip_interview" }).catch(() => undefined);
    return;
  }
  await acp.respond(id, {}).catch(() => undefined);
}

function permissionOptionIds(params: unknown): string[] {
  const rec =
    params && typeof params === "object" && !Array.isArray(params)
      ? (params as Record<string, unknown>)
      : null;
  const options = Array.isArray(rec?.options) ? rec.options : [];
  return options.flatMap((item) => {
    const id =
      item && typeof item === "object" && !Array.isArray(item)
        ? (item as Record<string, unknown>).optionId
        : undefined;
    return typeof id === "string" ? [id] : [];
  });
}

function textFromUpdate(params: unknown): string {
  const rec =
    params && typeof params === "object" && !Array.isArray(params)
      ? (params as Record<string, unknown>)
      : null;
  const update =
    rec?.update && typeof rec.update === "object" && !Array.isArray(rec.update)
      ? (rec.update as Record<string, unknown>)
      : rec;
  if (!update) return "";
  const kind = String(
    update.sessionUpdate ?? update.session_update ?? update.type ?? "",
  );
  if (kind !== "agent_message_chunk" && kind !== "agent_message") return "";
  return textFromContent(update.content ?? update.text);
}

function textFromContent(content: unknown): string {
  if (typeof content === "string") return content;
  if (content && typeof content === "object" && !Array.isArray(content)) {
    const rec = content as Record<string, unknown>;
    if (typeof rec.text === "string") return rec.text;
    if (rec.content != null) return textFromContent(rec.content);
  }
  if (Array.isArray(content)) {
    return content.map((item) => textFromContent(item)).join("");
  }
  return "";
}

function modelSettingsKey(settings?: Record<string, string>): string {
  return JSON.stringify({ effort: grokEffort(settings) ?? "low" });
}

import { AcpClient } from "../../core/acp";
import {
  killChild,
  resolveCursorBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import { abortTextPromptRace } from "../../core/abortTextPrompt";
import { mergeStream } from "../../core/streamText";

const TEXT_CHILD_ID = "monocode-text";
const INIT_TIMEOUT_MS = 60_000;
const REQUEST_TIMEOUT_MS = 20_000;
const TEXT_MODEL = "composer-2.5";

const CLIENT_CAPABILITIES = {
  fs: { readTextFile: false, writeTextFile: false },
  terminal: false,
  _meta: { parameterizedModelPicker: true },
};

type LiveText = {
  acp: AcpClient;
  cwd: string;
  model: string;
  settingsKey: string;
  acpSessionId: string;
  collecting: boolean;
  output: string;
  closed: boolean;
};

let live: LiveText | null = null;
let turns: Promise<void> = Promise.resolve();

export async function stopCursorTextPrompt(childId?: string): Promise<void> {
  await dropLive();
  if (childId && childId !== TEXT_CHILD_ID) {
    unwatchChild(childId);
    await killChild(childId).catch(() => undefined);
  }
}

/** Start the shared text ACP process in the background so the first prompt is fast. */
export function warmupCursorText(cwd: string): Promise<void> {
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

/** Cursor ACP turn in ask mode. Reuses a warm `cursor-agent acp` process. */
export async function runCursorTextPrompt(input: {
  cwd: string;
  model?: string;
  modelSettings?: Record<string, string>;
  prompt: string;
  timeoutMs?: number;
  signal?: AbortSignal;
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
}): Promise<string> {
  const session = await ensureLive(input.cwd, input.model, input.modelSettings);
  session.output = "";
  session.collecting = true;
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
  if (live && !live.closed) {
    if (
      live.cwd === cwd &&
      live.model === model &&
      live.settingsKey === settingsKey
    )
      return live;
    try {
      await openSession(live, cwd, model, modelSettings);
      return live;
    } catch {
      await dropLive();
    }
  }
  return startLive(cwd, model, modelSettings);
}

async function startLive(
  cwd: string,
  model = TEXT_MODEL,
  modelSettings?: Record<string, string>,
): Promise<LiveText> {
  await dropLive();
  const { path } = await resolveCursorBinary();
  const acpRef: { session: LiveText | null } = { session: null };
  const acp = new AcpClient(TEXT_CHILD_ID, {
    onNotification: (method, params) => {
      const session = acpRef.session;
      if (!session || method !== "session/update" || !session.collecting)
        return;
      session.output = mergeStream(session.output, textFromUpdate(params));
    },
    onRequest: (id, method, params) => {
      void handleTextRequest(acp, id, method, params);
    },
  });
  const session: LiveText = {
    acp,
    cwd,
    model,
    settingsKey: modelSettingsKey(modelSettings),
    acpSessionId: "",
    collecting: false,
    output: "",
    closed: false,
  };
  acpRef.session = session;

  watchChild(
    TEXT_CHILD_ID,
    (line) => acp.pushLine(line),
    () => {
      session.closed = true;
      if (live === session) live = null;
      acp.close(new Error("Cursor text generator exited"));
    },
  );

  try {
    await spawnChild(TEXT_CHILD_ID, path, ["acp"], cwd, undefined, "cursor");
    await acp.request(
      "initialize",
      {
        protocolVersion: 1,
        clientCapabilities: CLIENT_CAPABILITIES,
        clientInfo: { name: "monocode-text", version: "0.1.0" },
      },
      INIT_TIMEOUT_MS,
    );
    await acp
      .request("authenticate", { methodId: "cursor_login" }, REQUEST_TIMEOUT_MS)
      .catch(() => undefined);
    await openSession(session, cwd, model, modelSettings);
    live = session;
    return session;
  } catch (error) {
    session.closed = true;
    acp.close(error instanceof Error ? error : new Error(String(error)));
    unwatchChild(TEXT_CHILD_ID);
    await killChild(TEXT_CHILD_ID).catch(() => undefined);
    throw error;
  }
}

async function openSession(
  session: LiveText,
  cwd: string,
  model: string,
  modelSettings?: Record<string, string>,
): Promise<void> {
  const setup = await session.acp.request<{
    sessionId?: string;
    configOptions?: unknown;
  }>("session/new", { cwd, mcpServers: [] }, REQUEST_TIMEOUT_MS);
  const acpSessionId = setup.sessionId?.trim();
  if (!acpSessionId) throw new Error("Cursor did not return a session id");

  await session.acp
    .request(
      "session/set_mode",
      { sessionId: acpSessionId, modeId: "ask" },
      REQUEST_TIMEOUT_MS,
    )
    .catch(() => undefined);

  const modelConfigId = extractModelConfigId(setup.configOptions);
  await session.acp
    .request(
      "session/set_config_option",
      {
        sessionId: acpSessionId,
        configId: modelConfigId,
        value: model,
      },
      REQUEST_TIMEOUT_MS,
    )
    .catch(() =>
      session.acp
        .request(
          "session/set_model",
          { sessionId: acpSessionId, modelId: model },
          REQUEST_TIMEOUT_MS,
        )
        .catch(() => undefined),
    );

  for (const [settingId, value] of Object.entries(modelSettings ?? {})) {
    const configId = resolveSettingConfigId(setup.configOptions, settingId);
    if (!configId) continue;
    await session.acp
      .request(
        "session/set_config_option",
        {
          sessionId: acpSessionId,
          configId,
          value,
        },
        REQUEST_TIMEOUT_MS,
      )
      .catch(() => undefined);
  }

  session.cwd = cwd;
  session.model = model;
  session.settingsKey = modelSettingsKey(modelSettings);
  session.acpSessionId = acpSessionId;
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
  if (method === "cursor/ask_question") {
    await acp
      .respond(id, {
        outcome: {
          outcome: "skipped",
          reason: "Text generation does not answer questions",
        },
      })
      .catch(() => undefined);
    return;
  }
  await acp.respond(id, {}).catch(() => undefined);
}
function modelSettingsKey(settings?: Record<string, string>): string {
  return JSON.stringify(settings ?? {});
}

function resolveSettingConfigId(
  raw: unknown,
  settingId: string,
): string | undefined {
  const needle = settingId.trim().toLowerCase();
  const options = Array.isArray(raw)
    ? raw.flatMap((item) => {
        const rec = asRecord(item);
        const id = String(rec?.id ?? rec?.configId ?? "").trim();
        if (!id) return [];
        return [
          {
            id,
            category: String(rec?.category ?? "").trim(),
          },
        ];
      })
    : [];
  const exact = options.find((option) => option.id.toLowerCase() === needle);
  if (exact) return exact.id;
  if (needle === "effort" || needle === "reasoning") {
    return options.find(
      (option) =>
        option.id === "effort" ||
        option.id === "reasoning" ||
        (option.category === "thought_level" && option.id !== "thinking"),
    )?.id;
  }
  if (needle === "fast" || needle === "fastmode") {
    return options.find(
      (option) =>
        option.id === "fast" || option.id.toLowerCase().includes("fast"),
    )?.id;
  }
  if (needle === "thinking") {
    return options.find((option) => option.id === "thinking")?.id;
  }
  if (needle === "context" || needle === "contextwindow") {
    return options.find(
      (option) => option.id === "context" || option.id === "context_size",
    )?.id;
  }
  return undefined;
}

function permissionOptionIds(params: unknown): string[] {
  const rec = asRecord(params);
  const options = Array.isArray(rec?.options) ? rec.options : [];
  return options.flatMap((item) => {
    const id = asRecord(item)?.optionId;
    return typeof id === "string" ? [id] : [];
  });
}

function extractModelConfigId(raw: unknown): string {
  if (!Array.isArray(raw)) return "model";
  for (const item of raw) {
    const rec = asRecord(item);
    const id = String(rec?.id ?? rec?.configId ?? "").trim();
    const category = String(rec?.category ?? "").trim();
    if (id && (category === "model" || id === "model")) return id;
  }
  return "model";
}

function textFromUpdate(params: unknown): string {
  const rec = asRecord(params);
  const update = asRecord(rec?.update) ?? rec;
  if (!update) return "";
  const kind = String(
    update.sessionUpdate ?? update.session_update ?? update.type ?? "",
  );
  if (kind !== "agent_message_chunk" && kind !== "agent_message") return "";
  return textFromContent(update.content ?? update.text);
}

function textFromContent(content: unknown): string {
  if (typeof content === "string") return content;
  const rec = asRecord(content);
  if (rec && typeof rec.text === "string") return rec.text;
  if (rec && rec.content != null) return textFromContent(rec.content);
  if (Array.isArray(content)) {
    return content.map((item) => textFromContent(item)).join("");
  }
  return "";
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

import { AcpClient } from "../../core/acp";
import {
  killChild,
  resolveGrokBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import {
  grokAuthMethodId,
  grokTextSpawnArgs,
  TEXT_MODEL,
} from "./grokProtocol";
import { mergeStream } from "../../core/streamText";

const TEXT_CHILD_ID = "monocode-grok-text";
const INIT_TIMEOUT_MS = 60_000;
const REQUEST_TIMEOUT_MS = 20_000;

const CLIENT_CAPABILITIES = {
  fs: { readTextFile: false, writeTextFile: false },
  terminal: false,
};

type LiveText = {
  acp: AcpClient;
  cwd: string;
  acpSessionId: string;
  collecting: boolean;
  output: string;
  closed: boolean;
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
  const run = turns.catch(() => undefined).then(async () => {
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
  prompt: string;
  timeoutMs: number;
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
  prompt: string;
  timeoutMs: number;
}): Promise<string> {
  const session = await ensureLive(input.cwd);
  session.output = "";
  session.collecting = true;
  try {
    await session.acp.request(
      "session/prompt",
      {
        sessionId: session.acpSessionId,
        prompt: [{ type: "text", text: input.prompt }],
      },
      input.timeoutMs,
    );
    return session.output;
  } catch (error) {
    await session.acp
      .notify("session/cancel", { sessionId: session.acpSessionId })
      .catch(() => undefined);
    if (session.closed) await dropLive();
    throw error;
  } finally {
    session.collecting = false;
    await dropLive();
  }
}

async function ensureLive(cwd: string): Promise<LiveText> {
  if (live && !live.closed) {
    if (live.cwd === cwd) return live;
    try {
      await openSession(live, cwd);
      return live;
    } catch {
      await dropLive();
    }
  }
  return startLive(cwd);
}

async function startLive(cwd: string): Promise<LiveText> {
  await dropLive();
  const { path } = await resolveGrokBinary();
  const acpRef: { session: LiveText | null } = { session: null };
  const acp = new AcpClient(TEXT_CHILD_ID, {
    onNotification: (method, params) => {
      const session = acpRef.session;
      if (!session || method !== "session/update" || !session.collecting) return;
      session.output = mergeStream(session.output, textFromUpdate(params));
    },
    onRequest: (id, method, params) => {
      void handleTextRequest(acp, id, method, params);
    },
  });
  const session: LiveText = {
    acp,
    cwd,
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
    await openSession(session, cwd);
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

async function openSession(session: LiveText, cwd: string): Promise<void> {
  const setup = await session.acp.request<{ sessionId?: string }>(
    "session/new",
    { cwd, mcpServers: [] },
    REQUEST_TIMEOUT_MS,
  );
  const acpSessionId = setup.sessionId?.trim();
  if (!acpSessionId) throw new Error("Grok Build did not return a session id");

  await session.acp
    .request(
      "session/set_model",
      { sessionId: acpSessionId, modelId: TEXT_MODEL },
      REQUEST_TIMEOUT_MS,
    )
    .catch(() => undefined);
  await session.acp
    .request(
      "session/set_mode",
      { sessionId: acpSessionId, modeId: "low" },
      REQUEST_TIMEOUT_MS,
    )
    .catch(() => undefined);

  session.cwd = cwd;
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
  if (
    method === "_x.ai/ask_user_question" ||
    method === "x.ai/ask_user_question"
  ) {
    await acp
      .respond(id, { outcome: "skip_interview" })
      .catch(() => undefined);
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

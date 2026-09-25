import { modelsFor } from "../../../../features/sessions/model/models";
import type { TurnIntent } from "../../../../features/sessions/model/session";
import { abortTextPromptRace } from "../../core/abortTextPrompt";
import {
  execChild,
  freeHarnessPort,
  killChild,
  resolveOpenCodeBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import { OpenCodeClient } from "./opencodeClient";
import {
  assertSupportedOpenCodeVersion,
  parseOpenCodeModelSlug,
  parseOpenCodeVersion,
  parseServerUrlFromOutput,
} from "./opencodeProtocol";
import { resolveOpenCodeV2Service } from "./opencodeService";

const TEXT_CHILD_ID = "monocode-opencode-text";
const SERVER_TIMEOUT_MS = 30_000;
const REQUEST_TIMEOUT_MS = 45_000;

type LiveText = {
  client: OpenCodeClient;
  sessionId: string;
  cwd: string;
  model: { providerID: string; modelID: string };
  modelSettingsKey: string;
  modelSettings?: Record<string, string>;
};

let live: LiveText | null = null;
let turns: Promise<void> = Promise.resolve();
let serverUrl = "";

export async function stopOpenCodeTextPrompt(): Promise<void> {
  await dropLive();
}

export function warmupOpenCodeText(cwd: string): Promise<void> {
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

export async function runOpenCodeTextPrompt(input: {
  cwd: string;
  model?: string;
  modelSettings?: Record<string, string>;
  intent?: TurnIntent;
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
  intent?: TurnIntent;
  prompt: string;
  timeoutMs?: number;
  signal?: AbortSignal;
}): Promise<string> {
  const session = await ensureLive(input.cwd, input.model, input.modelSettings);
  const abort = abortTextPromptRace(input.signal, () =>
    session.client.abortSession(session.sessionId),
  );
  try {
    const result = await Promise.race([
      session.client.prompt({
        sessionID: session.sessionId,
        model: session.model,
        agent: openCodeTextAgent(input.intent, session.modelSettings),
        variant: session.modelSettings?.variant,
        parts: [{ type: "text", text: input.prompt }],
        timeoutMs: input.timeoutMs ?? REQUEST_TIMEOUT_MS,
      }),
      ...(abort.promise ? [abort.promise] : []),
    ]);
    const error = result.info?.error;
    if (error) {
      throw new Error(
        typeof error === "object" && error && "message" in error
          ? String((error as { message: unknown }).message)
          : "OpenCode text generation failed",
      );
    }
    const text = getOpenCodeTextResponse(result.parts);
    if (!text) throw new Error("OpenCode returned empty output.");
    return text;
  } finally {
    abort.detach();
    await dropLive();
  }
}

async function ensureLive(
  cwd: string,
  requestedModel?: string,
  modelSettings?: Record<string, string>,
): Promise<LiveText> {
  const model = pickTextModel(requestedModel);
  const settingsKey = modelSettingsKey(modelSettings);
  if (
    live &&
    live.cwd === cwd &&
    sameModel(live.model, model) &&
    live.modelSettingsKey === settingsKey
  )
    return live;
  if (live) await dropLive();
  return startLive(cwd, model, modelSettings);
}

async function startLive(
  cwd: string,
  model: { providerID: string; modelID: string },
  modelSettings?: Record<string, string>,
): Promise<LiveText> {
  const { path } = await resolveOpenCodeBinary();
  const versionOut = await execChild(path, ["--version"], cwd).catch(() => "");
  const version = parseOpenCodeVersion(versionOut);
  const generation = assertSupportedOpenCodeVersion(version);

  const service =
    generation === "v2"
      ? await resolveOpenCodeV2Service(path, cwd)
      : undefined;
  serverUrl = service?.url ?? "";
  if (generation === "v1") {
    watchChild(
      TEXT_CHILD_ID,
      (line) => {
        const parsed = parseServerUrlFromOutput(line);
        if (parsed) serverUrl = parsed;
      },
      () => {
        if (live) live = null;
      },
      (line) => {
        const parsed = parseServerUrlFromOutput(line);
        if (parsed) serverUrl = parsed;
      },
    );

    const port = await freeHarnessPort();
    await spawnChild(
      TEXT_CHILD_ID,
      path,
      ["serve", `--hostname=127.0.0.1`, `--port=${port}`],
      cwd,
    );
  }

  try {
    const url =
      generation === "v2"
        ? serverUrl
        : await waitForUrl(() => serverUrl, SERVER_TIMEOUT_MS);
    const client = new OpenCodeClient(
      url,
      cwd,
      generation,
      service?.password,
    );
    const created = await client.createSession({
      permission: [{ permission: "*", pattern: "*", action: "deny" }],
    });
    live = {
      client,
      sessionId: created.id,
      cwd,
      model,
      modelSettingsKey: modelSettingsKey(modelSettings),
      modelSettings,
    };
    return live;
  } catch (error) {
    await dropLive();
    throw error;
  }
}

async function dropLive(): Promise<void> {
  const current = live;
  live = null;
  if (current) {
    await current.client.abortSession(current.sessionId);
    await current.client.closeEvents(TEXT_CHILD_ID);
  }
  unwatchChild(TEXT_CHILD_ID);
  await killChild(TEXT_CHILD_ID).catch(() => undefined);
}

function pickTextModel(requested?: string): {
  providerID: string;
  modelID: string;
} {
  const selected = requested?.trim();
  if (selected) {
    const modelSlug = selected.startsWith("opencode:")
      ? selected.slice("opencode:".length)
      : selected;
    const parsedSelected = parseOpenCodeModelSlug(modelSlug);
    if (parsedSelected) return parsedSelected;
    if (modelSlug) return { providerID: "opencode", modelID: modelSlug };
  }
  const models = modelsFor("opencode");
  for (const model of models) {
    const parsed = parseOpenCodeModelSlug(model.nativeId ?? model.id);
    if (parsed) return parsed;
  }
  return { providerID: "opencode", modelID: "glm-5" };
}

function sameModel(
  left: { providerID: string; modelID: string },
  right: { providerID: string; modelID: string },
): boolean {
  return left.providerID === right.providerID && left.modelID === right.modelID;
}

function modelSettingsKey(settings?: Record<string, string>): string {
  return JSON.stringify({
    agent: settings?.agent,
    variant: settings?.variant,
  });
}

function openCodeTextAgent(
  intent: TurnIntent | undefined,
  settings?: Record<string, string>,
): string | undefined {
  if (intent === "plan") return "plan";
  if (intent === "build") return "build";
  const configured = settings?.agent?.trim();
  return configured || "build";
}

export function getOpenCodeTextResponse(parts: unknown[] | undefined): string {
  return (parts ?? [])
    .flatMap((part) => {
      if (!part || typeof part !== "object") return [];
      if (!("type" in part) || part.type !== "text") return [];
      if (!("text" in part) || typeof part.text !== "string") return [];
      return [part.text];
    })
    .join("")
    .trim();
}

function waitForUrl(read: () => string, timeoutMs: number): Promise<string> {
  return new Promise((resolve, reject) => {
    const started = Date.now();
    const tick = () => {
      const url = read();
      if (url) {
        resolve(url);
        return;
      }
      if (Date.now() - started >= timeoutMs) {
        reject(new Error("Timed out waiting for OpenCode text server"));
        return;
      }
      setTimeout(tick, 50);
    };
    tick();
  });
}

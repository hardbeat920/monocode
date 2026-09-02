import { modelsFor } from "../models";
import {
  killChild,
  resolveAntigravityBinary,
  spawnChild,
  unwatchChild,
  watchChild,
  writeChild,
} from "./child";
import {
  buildAntigravitySpawnArgs,
  buildAntigravityUserMessage,
  parseAntigravityLine,
  stringField,
  asRecord,
} from "./antigravityProtocol";
import { mergeStream } from "./streamText";

const TEXT_CHILD_ID = "monocode-antigravity-text";
const INIT_TIMEOUT_MS = 90_000;
const REQUEST_TIMEOUT_MS = 120_000;
const TEXT_MODEL = "gemini-3.8-flash-high";

type LiveText = {
  cwd: string;
  collecting: boolean;
  output: string;
  closed: boolean;
  ready: boolean;
  turnDone: (() => void) | null;
  turnFailed: ((error: Error) => void) | null;
  readyDone: (() => void) | null;
};

let live: LiveText | null = null;
let turns: Promise<void> = Promise.resolve();

function pickTextModel(): string {
  const models = modelsFor("antigravity");
  const flash = models.find((model) =>
    /flash/i.test(`${model.nativeId ?? ""} ${model.name} ${model.id}`),
  );
  return flash?.nativeId ?? TEXT_MODEL;
}

export async function stopAntigravityTextPrompt(): Promise<void> {
  await dropLive();
}

export function warmupAntigravityText(cwd: string): Promise<void> {
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

export async function runAntigravityTextPrompt(input: {
  cwd: string;
  prompt: string;
  timeoutMs?: number;
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
  timeoutMs?: number;
}): Promise<string> {
  const session = await ensureLive(input.cwd);
  session.output = "";
  session.collecting = true;
  const timeoutMs = input.timeoutMs ?? REQUEST_TIMEOUT_MS;

  try {
    const turnPromise = new Promise<void>((resolve, reject) => {
      session.turnDone = resolve;
      session.turnFailed = reject;
    });

    const message = buildAntigravityUserMessage({ text: input.prompt });
    if (!message) throw new Error("Empty prompt for Antigravity text generator");

    await writeChild(TEXT_CHILD_ID, JSON.stringify(message));

    await Promise.race([
      turnPromise,
      new Promise<void>((_, reject) => {
        setTimeout(
          () => reject(new Error("Antigravity text generation timed out")),
          timeoutMs,
        );
      }),
    ]);

    const output = session.output.trim();
    if (!output) throw new Error("Antigravity returned empty output.");
    return output;
  } catch (error) {
    if (session.closed) await dropLive();
    throw error;
  } finally {
    session.collecting = false;
    session.turnDone = null;
    session.turnFailed = null;
    await dropLive();
  }
}

async function ensureLive(cwd: string): Promise<LiveText> {
  if (live && !live.closed && live.cwd === cwd) return live;
  await dropLive();
  return startLive(cwd);
}

async function startLive(cwd: string): Promise<LiveText> {
  const { path } = await resolveAntigravityBinary();
  const session: LiveText = {
    cwd,
    collecting: false,
    output: "",
    closed: false,
    ready: false,
    turnDone: null,
    turnFailed: null,
    readyDone: null,
  };

  watchChild(
    TEXT_CHILD_ID,
    (line) => handleLine(session, line),
    () => {
      session.closed = true;
      if (live === session) live = null;
      session.turnFailed?.(new Error("Antigravity text generator exited"));
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
      buildAntigravitySpawnArgs({
        model: pickTextModel(),
        runtimeMode: "supervised",
        cwd,
      }),
      cwd,
    );
    live = session;
    await waitForReady(session, INIT_TIMEOUT_MS);
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
    current.turnFailed?.(new Error("Antigravity text generator stopped"));
    current.turnDone = null;
    current.turnFailed = null;
    current.readyDone = null;
  }
  unwatchChild(TEXT_CHILD_ID);
  await killChild(TEXT_CHILD_ID).catch(() => undefined);
}

function handleLine(session: LiveText, line: string): void {
  const rec = parseAntigravityLine(line);
  if (!rec) return;
  const event = stringField(rec, "event");
  if (event === "init") {
    session.ready = true;
    session.readyDone?.();
    session.readyDone = null;
  }
  if (!session.collecting) return;
  if (event === "step_update") {
    const step = asRecord(rec.step_update);
    const type = stringField(step, "step_type");
    if (type === "agent_response") {
      const delta = stringField(step, "text_delta");
      if (delta) session.output = mergeStream(session.output, delta);
    }
    return;
  }
  if (event === "result") {
    const result = asRecord(rec.result);
    const status = stringField(result, "status")?.toUpperCase();
    const response = stringField(result, "response");
    const error = stringField(result, "error");
    if (response && !session.output.trim()) {
      session.output = response;
    }
    if (status === "SUCCESS") {
      session.turnDone?.();
    } else {
      session.turnFailed?.(
        new Error(error ?? "Antigravity text generation turn failed"),
      );
    }
    session.turnDone = null;
    session.turnFailed = null;
  }
}

function waitForReady(session: LiveText, timeoutMs: number): Promise<void> {
  if (session.ready) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      session.readyDone = null;
      resolve();
    }, timeoutMs);
    session.readyDone = () => {
      clearTimeout(timer);
      if (session.closed) {
        reject(new Error("Antigravity text generator exited"));
        return;
      }
      resolve();
    };
  });
}

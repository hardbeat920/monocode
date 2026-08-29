import { modelsFor } from "../models";
import {
  killChild,
  resolveClaudeBinary,
  spawnChild,
  unwatchChild,
  watchChild,
  writeChild,
} from "./child";
import {
  assistantTextBlocks,
  buildClaudeSpawnArgs,
  buildClaudeUserMessage,
  parseJsonLine,
  stringField,
  turnStatusFromResult,
} from "./claudeProtocol";
import { mergeStream } from "./streamText";

const TEXT_CHILD_ID = "monocode-claude-text";
const INIT_TIMEOUT_MS = 8_000;
const REQUEST_TIMEOUT_MS = 45_000;
const TEXT_MODEL = "claude-haiku-4-5";

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
  const run = turns.catch(() => undefined).then(async () => {
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

    await writeChild(
      TEXT_CHILD_ID,
      JSON.stringify(buildClaudeUserMessage({ text: input.prompt })),
    );

    await Promise.race([
      turnPromise,
      new Promise<void>((_, reject) => {
        setTimeout(
          () => reject(new Error("Claude text generation timed out")),
          timeoutMs,
        );
      }),
    ]);

    const output = session.output.trim();
    if (!output) throw new Error("Claude returned empty output.");
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
  const { path } = await resolveClaudeBinary();
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
        model: pickTextModel(),
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
  if (
    type === "system" &&
    (stringField(rec, "subtype") === "init" ||
      stringField(rec, "subtype") === "initialized")
  ) {
    session.ready = true;
    session.readyDone?.();
    session.readyDone = null;
  }
  if (!session.collecting) return;
  if (type === "assistant") {
    const snapshot = assistantTextBlocks(rec).join("");
    if (snapshot) session.output = mergeStream(session.output, snapshot);
    return;
  }
  if (type === "stream_event") {
    const event = rec.event;
    if (!event || typeof event !== "object") return;
    const delta = (event as { delta?: { type?: string; text?: string } }).delta;
    if (delta?.type === "text_delta" && typeof delta.text === "string") {
      session.output = mergeStream(session.output, delta.text);
    }
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
        reject(new Error("Claude text generator exited"));
        return;
      }
      resolve();
    };
  });
}

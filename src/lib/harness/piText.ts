import { modelsFor } from "../models";
import {
  killChild,
  spawnChild,
  unwatchChild,
  watchChild,
} from "./child";
import { PiRpc } from "./piClient";
import { OMP_FLAVOR, PI_FLAVOR, type PiFlavor } from "./piFlavor";
import {
  agentEndWillRetry,
  asRecord,
  assistantDeltaFromEvent,
  buildPiPrompt,
  buildPiSpawnArgs,
  isAgentSettled,
} from "./piProtocol";
import { mergeStream } from "./streamText";

const INIT_TIMEOUT_MS = 15_000;
const REQUEST_TIMEOUT_MS = 45_000;

type LiveText = {
  rpc: PiRpc;
  cwd: string;
  collecting: boolean;
  output: string;
  closed: boolean;
  turnDone: (() => void) | null;
  turnFailed: ((error: Error) => void) | null;
  turnEndPending: boolean;
};

type TextState = {
  live: LiveText | null;
  turns: Promise<void>;
};

const stateByFlavor = new Map<string, TextState>();

function stateFor(flavor: PiFlavor): TextState {
  let state = stateByFlavor.get(flavor.id);
  if (!state) {
    state = { live: null, turns: Promise.resolve() };
    stateByFlavor.set(flavor.id, state);
  }
  return state;
}

function pickTextModel(flavor: PiFlavor): string | undefined {
  const models = modelsFor(flavor.id).filter((model) =>
    Boolean(model.nativeId?.includes("/")),
  );
  const cheap = models.find((model) =>
    /haiku|mini|flash|nano|lite|luna/i.test(
      `${model.nativeId ?? ""} ${model.name} ${model.id}`,
    ),
  );
  return (cheap ?? models[0])?.nativeId?.trim() || undefined;
}

export async function stopTextPrompt(flavor: PiFlavor): Promise<void> {
  await dropLive(flavor);
}

export function warmupText(flavor: PiFlavor, cwd: string): Promise<void> {
  if (!cwd || cwd === "~") return Promise.resolve();
  const state = stateFor(flavor);
  const run = state.turns.catch(() => undefined).then(async () => {
    await ensureLive(flavor, cwd);
  });
  state.turns = run.then(
    () => undefined,
    () => undefined,
  );
  return run.catch(() => undefined);
}

export async function runTextPrompt(
  flavor: PiFlavor,
  input: {
    cwd: string;
    prompt: string;
    timeoutMs?: number;
  },
): Promise<string> {
  const state = stateFor(flavor);
  const run = state.turns
    .catch(() => undefined)
    .then(() => promptOnLive(flavor, input));
  state.turns = run.then(
    () => undefined,
    () => undefined,
  );
  return run;
}

async function promptOnLive(
  flavor: PiFlavor,
  input: {
    cwd: string;
    prompt: string;
    timeoutMs?: number;
  },
): Promise<string> {
  const session = await ensureLive(flavor, input.cwd);
  const timeoutMs = input.timeoutMs ?? REQUEST_TIMEOUT_MS;

  try {
    await session.rpc.request({ type: "new_session" }).catch(() => undefined);
    await session.rpc
      .request({ type: "set_thinking_level", level: "off" })
      .catch(() => undefined);

    session.output = "";
    session.collecting = true;
    session.turnEndPending = false;

    const turnPromise = new Promise<void>((resolve, reject) => {
      session.turnDone = resolve;
      session.turnFailed = reject;
    });

    await session.rpc.request(buildPiPrompt({ text: input.prompt }), timeoutMs);
    if (session.turnEndPending) finishTurn(session);

    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([
        turnPromise,
        new Promise<void>((_, reject) => {
          timer = setTimeout(
            () =>
              reject(new Error(`${flavor.label} text generation timed out`)),
            timeoutMs,
          );
        }),
      ]);
    } finally {
      if (timer) clearTimeout(timer);
    }

    let output = session.output.trim();
    if (!output) {
      const rec = await session.rpc
        .request({ type: "get_last_assistant_text" })
        .catch(() => null);
      const text = asRecord(rec?.data)?.text;
      if (typeof text === "string") output = text.trim();
    }
    if (!output) throw new Error(`${flavor.label} returned empty output.`);
    await dropLive(flavor);
    return output;
  } catch (error) {
    session.turnDone = null;
    session.turnFailed = null;
    await session.rpc.request({ type: "abort" }).catch(() => undefined);
    await dropLive(flavor);
    throw error;
  } finally {
    session.collecting = false;
    session.turnDone = null;
    session.turnFailed = null;
  }
}

async function ensureLive(flavor: PiFlavor, cwd: string): Promise<LiveText> {
  const state = stateFor(flavor);
  const current = state.live;
  if (current && !current.closed && current.cwd === cwd) return current;
  await dropLive(flavor);
  return startLive(flavor, cwd);
}

async function startLive(flavor: PiFlavor, cwd: string): Promise<LiveText> {
  const state = stateFor(flavor);
  const childId = flavor.textChildId;
  const { path } = await flavor.resolveBinary();
  const liveRef: { current: LiveText | null } = { current: null };
  const rpc = new PiRpc(
    childId,
    (rec) => {
      const current = liveRef.current;
      if (current) handleFrame(current, rec);
    },
    flavor.label,
  );
  const session: LiveText = {
    rpc,
    cwd,
    collecting: false,
    output: "",
    closed: false,
    turnDone: null,
    turnFailed: null,
    turnEndPending: false,
  };
  liveRef.current = session;

  watchChild(
    childId,
    (line) => rpc.pushLine(line),
    () => {
      session.closed = true;
      if (state.live === session) state.live = null;
      const exited = new Error(`${flavor.label} text generator exited`);
      rpc.close(exited);
      session.turnFailed?.(exited);
      session.turnDone = null;
      session.turnFailed = null;
    },
  );

  try {
    await spawnChild(
      childId,
      path,
      buildPiSpawnArgs(flavor, {
        isolated: true,
        model: pickTextModel(flavor),
      }),
      cwd,
    );
    await rpc.request({ type: "get_state" }, INIT_TIMEOUT_MS);
    state.live = session;
    return session;
  } catch (error) {
    session.closed = true;
    rpc.close(error instanceof Error ? error : new Error(String(error)));
    unwatchChild(childId);
    await killChild(childId).catch(() => undefined);
    throw error;
  }
}

async function dropLive(flavor: PiFlavor): Promise<void> {
  const state = stateFor(flavor);
  const current = state.live;
  const childId = flavor.textChildId;
  state.live = null;
  if (current) {
    current.closed = true;
    current.rpc.close();
    current.turnFailed?.(
      new Error(`${flavor.label} text generator stopped`),
    );
    current.turnDone = null;
    current.turnFailed = null;
  }
  unwatchChild(childId);
  await killChild(childId).catch(() => undefined);
}

function handleFrame(session: LiveText, rec: Record<string, unknown>) {
  if (!session.collecting) return;
  const delta = assistantDeltaFromEvent(rec);
  if (delta?.kind === "text") {
    session.output = mergeStream(session.output, delta.text);
  }
  if (isAgentSettled(rec) || agentEndWillRetry(rec) === false) {
    if (session.turnDone) finishTurn(session);
    else session.turnEndPending = true;
  }
}

function finishTurn(session: LiveText) {
  session.turnEndPending = false;
  const done = session.turnDone;
  session.turnDone = null;
  session.turnFailed = null;
  done?.();
}

export const stopPiTextPrompt = () => stopTextPrompt(PI_FLAVOR);
export const warmupPiText = (cwd: string) => warmupText(PI_FLAVOR, cwd);
export const runPiTextPrompt = (input: {
  cwd: string;
  prompt: string;
  timeoutMs?: number;
}) => runTextPrompt(PI_FLAVOR, input);

export const stopOmpTextPrompt = () => stopTextPrompt(OMP_FLAVOR);
export const warmupOmpText = (cwd: string) => warmupText(OMP_FLAVOR, cwd);
export const runOmpTextPrompt = (input: {
  cwd: string;
  prompt: string;
  timeoutMs?: number;
}) => runTextPrompt(OMP_FLAVOR, input);

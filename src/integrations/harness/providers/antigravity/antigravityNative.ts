import { invoke, isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { IS_WIN } from "../../../../platform/tauri/platform";
import { REMOTE_PATH_PREFIX } from "../../../../shared/lib/remotePaths";
import { deleteGeneratedImages, saveGeneratedImage } from "../../../../platform/tauri/fs";
import { setHarnessModels, type AgentModel } from "../../../../features/sessions/model/models";
import { hasHeadlessChildBackend } from "../../core/child";
import type { ApprovalDecision, HarnessEvent, SendTurnInput } from "../../core/types";

// Callers must supply their workspace context; undefined denotes a local,
// account-level action without an open workspace.
export function usesNativeAntigravity(cwd: string | undefined): boolean {
  return IS_WIN && !cwd?.startsWith(REMOTE_PATH_PREFIX) &&
    isTauri() && !hasHeadlessChildBackend();
}

export type AntigravityAccountStatus = {
  backendAvailable: boolean;
  authenticated: boolean;
  email?: string | null;
  loginPending: boolean;
  authError?: string | null;
};

function nativeError(error: unknown): Error {
  if (error instanceof Error) return error;
  if (typeof error === "object" && error && "message" in error)
    return new Error(String(error.message));
  return new Error(String(error));
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try { return await invoke<T>(command, args); }
  catch (error) { throw nativeError(error); }
}

export const nativeAntigravityAccount = () =>
  call<AntigravityAccountStatus>("antigravity_native_status");
export const loginNativeAntigravity = () => call<void>("antigravity_native_login");
export const cancelNativeAntigravityLogin = () => call<void>("antigravity_native_cancel_login");
export const logoutNativeAntigravity = () => call<void>("antigravity_native_logout");

export async function refreshNativeAntigravityCatalog(): Promise<void> {
  const models = await call<AgentModel[]>("antigravity_native_catalog");
  if (models.length === 0) throw new Error("Antigravity returned no compatible models.");
  setHarnessModels("antigravity", models);
}

type NativeEvent = {
  sessionId: string;
  turnId: string;
  event: HarnessEvent | { type: "turn.accepted" };
};
type Live = {
  turnId?: string;
  cancelled: boolean;
  epoch: number;
  tail: Promise<void>;
  binding?: Promise<void>;
  bindingError?: Error;
};
const sessions = new Map<string, Live>();

function session(id: string): Live {
  let current = sessions.get(id);
  if (!current) {
    current = { cancelled: false, epoch: 0, tail: Promise.resolve() };
    sessions.set(id, current);
  }
  return current;
}

export const hasNativeAntigravitySession = (id: string) => sessions.has(id);

export async function sendNativeAntigravityTurn(input: SendTurnInput): Promise<void> {
  const live = session(input.sessionId);
  const epoch = live.epoch;
  const run = live.tail.catch(() => undefined).then(async () => {
    if (live.epoch !== epoch || sessions.get(input.sessionId) !== live) return;
    await live.binding;
    if (live.bindingError) throw live.bindingError;
    if (live.epoch !== epoch) return;
    const turnId = crypto.randomUUID();
    live.turnId = turnId;
    live.cancelled = false;
    let accepted = false;
    let plan = "";
    const images: Promise<void>[] = [];
    const current = () => live.epoch === epoch && live.turnId === turnId &&
      !live.cancelled && sessions.get(input.sessionId) === live;
    // Listen on the owning window before invoking. Rust targets this same
    // window, and ids reject late events from retired turns and sessions.
    let unlisten: (() => void) | undefined;
    try {
      unlisten = await getCurrentWindow().listen<NativeEvent>(
        "antigravity-native-event",
        ({ payload }) => {
          if (payload.sessionId !== input.sessionId || payload.turnId !== turnId ||
              live.turnId !== turnId || live.cancelled || sessions.get(input.sessionId) !== live) return;
          if (payload.event.type === "turn.accepted") {
            if (!accepted) { accepted = true; input.onAccepted?.(); }
            return;
          }
          if (payload.event.type === "image.generated" && "data" in payload.event) {
            const event = payload.event;
            images.push((async () => {
              const asset = await saveGeneratedImage({ data: event.data, name: event.name });
              if (!current()) { await deleteGeneratedImages([asset.path]); return; }
              input.onEvent({ type: "image.generated", itemId: event.itemId, ...asset, name: event.name, alt: event.alt });
            })().catch((error) => { if (current()) input.onEvent({ type: "session.error", message: nativeError(error).message }); }));
            return;
          }
          if (payload.event.type === "turn.ready" && images.length) {
            void Promise.all(images).then(() => { if (current()) input.onEvent({ type: "turn.ready" }); });
            return;
          }
          if (input.intent === "plan" && payload.event.type === "message.delta") {
            plan += payload.event.text;
            input.onEvent({ type: "plan", text: payload.event.text, key: turnId, append: true, streaming: true });
          } else if (input.intent === "plan" && payload.event.type === "message.completed") {
            input.onEvent({ type: "plan", text: plan, key: turnId, streaming: false });
          } else input.onEvent(payload.event);
        },
      );
      if (live.epoch !== epoch) return;
      await call<void>("antigravity_native_send", {
        input: {
          sessionId: input.sessionId, turnId, cwd: input.cwd,
          model: input.model, modelSettings: input.modelSettings ?? {},
          runtimeMode: input.runtimeMode, intent: input.intent,
          text: input.text,
          attachments: (input.attachments ?? []).map(({ name, mimeType, data, path }) =>
            ({ name, mimeType, data, path })),
        },
      });
      await Promise.all(images);
    } catch (error) {
      if (!live.cancelled && live.epoch === epoch) throw error;
    } finally {
      unlisten?.();
      if (live.turnId === turnId) live.turnId = undefined;
    }
  });
  live.tail = run;
  await run;
}

export function approveNativeAntigravity(sessionId: string, requestId: number, decision: ApprovalDecision): void {
  const turnId = sessions.get(sessionId)?.turnId;
  if (!turnId) return;
  void call<void>("antigravity_native_approve", { sessionId, turnId, requestId, decision })
    .catch((error) => console.debug("[monocode] Antigravity approval failed", error.message));
}

export async function cancelNativeAntigravity(sessionId: string): Promise<void> {
  const live = sessions.get(sessionId);
  if (!live) return;
  live.cancelled = true;
  live.epoch += 1;
  await call<void>("antigravity_native_cancel", { sessionId, turnId: live.turnId });
}

export async function stopNativeAntigravity(sessionId: string, forget = false): Promise<void> {
  const live = sessions.get(sessionId);
  if (live) { live.cancelled = true; live.epoch += 1; }
  if (live?.turnId) await call<void>("antigravity_native_cancel", { sessionId, turnId: live.turnId });
  await live?.tail.catch(() => undefined);
  await call<void>("antigravity_native_stop", { sessionId, forget });
  if (forget && sessions.get(sessionId) === live) sessions.delete(sessionId);
}

export function bindNativeAntigravity(sessionId: string, providerSessionId: string, cwd: string): void {
  const live = session(sessionId);
  live.bindingError = undefined;
  live.binding = call<void>("antigravity_native_bind", { sessionId, providerSessionId, cwd })
    .catch((error: Error) => { live.bindingError = error; });
}

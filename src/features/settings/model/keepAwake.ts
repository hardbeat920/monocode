import { invoke, isTauri } from "@tauri-apps/api/core";
import { useEffect, useRef, useSyncExternalStore } from "react";
import { sessionNeedsInput, type Session } from "../../sessions/model/session";
import {
  KEEP_AWAKE_HOLD_AFTER_DEFAULT,
  keepAwakeHoldAfterMs,
  loadKeepAwakeEnabled,
  loadKeepAwakeHoldAfter,
  loadKeepAwakeScreen,
  subscribeKeepAwakeEnabled,
  subscribeKeepAwakeHoldAfter,
  subscribeKeepAwakeScreen,
} from "./settings";

// React StrictMode can replace a controller before its final IPC settles.
let nativeQueue: Promise<void> = Promise.resolve();

function sendNative(enabled: boolean, display = false): Promise<void> {
  const operation = nativeQueue.then(() =>
    invoke<void>("set_keep_awake", { enabled, display }),
  );
  nativeQueue = operation.then(
    () => undefined,
    () => undefined,
  );
  return operation;
}

/** A pending approval/question is not active work even when busy is still set. */
export function isWorkingSession(session: Session): boolean {
  return (
    !!session.busy && !session.worktreeRemoved && !sessionNeedsInput(session)
  );
}

type KeepAwakePhase = "off" | "working" | "holding";

/** Each webview reports only its own activity; the native command combines windows. */
export function createKeepAwakeController(
  send: (enabled: boolean, display?: boolean) => Promise<unknown>,
) {
  let desired = false;
  let desiredDisplay = false;
  let disposed = false;
  let failed = false;
  let pending = Promise.resolve();
  let phase: KeepAwakePhase = "off";
  let holdSince = 0;
  let holdTimer: ReturnType<typeof setTimeout> | null = null;
  let keepScreen = false;

  const clearTimer = () => {
    if (holdTimer == null) return;
    clearTimeout(holdTimer);
    holdTimer = null;
  };

  const set = (enabled: boolean, display = false) => {
    if (desired === enabled && desiredDisplay === display && !failed) return;
    desired = enabled;
    desiredDisplay = display;
    failed = false;
    // Preserve transition order if a turn finishes before the first IPC returns.
    pending = pending.then(async () => {
      try {
        await send(enabled, display);
      } catch {
        if (desired !== enabled || desiredDisplay !== display) return;
        try {
          await send(enabled, display);
        } catch {
          if (desired === enabled && desiredDisplay === display) failed = true;
        }
      }
    });
  };

  const applyHold = (holdAfterMs: number) => {
    if (holdAfterMs <= 0) {
      phase = "off";
      clearTimer();
      set(false);
      return;
    }
    if (!Number.isFinite(holdAfterMs)) {
      clearTimer();
      set(true, keepScreen);
      return;
    }
    const remaining = holdSince + holdAfterMs - Date.now();
    if (remaining <= 0) {
      phase = "off";
      clearTimer();
      set(false);
      return;
    }
    clearTimer();
    holdTimer = setTimeout(() => {
      holdTimer = null;
      phase = "off";
      set(false);
    }, remaining);
    set(true, keepScreen);
  };

  return {
    update(
      enabled: boolean,
      sessions: readonly Session[],
      holdAfterMs = 0,
      screen = false,
    ) {
      if (disposed) return;
      keepScreen = screen;
      const busy = sessions.some(isWorkingSession);
      if (!enabled) {
        phase = "off";
        clearTimer();
        set(false);
        return;
      }
      if (busy) {
        phase = "working";
        clearTimer();
        set(true, keepScreen);
        return;
      }
      if (phase === "working") {
        phase = "holding";
        holdSince = Date.now();
      }
      if (phase !== "holding") {
        set(false);
        return;
      }
      applyHold(holdAfterMs);
    },
    release() {
      if (disposed) return;
      phase = "off";
      clearTimer();
      set(false);
      disposed = true;
    },
    settled: () => pending,
  };
}

export function useKeepAwake(sessions: readonly Session[]): void {
  const enabled = useSyncExternalStore(
    subscribeKeepAwakeEnabled,
    loadKeepAwakeEnabled,
    () => false,
  );
  const holdAfter = useSyncExternalStore(
    subscribeKeepAwakeHoldAfter,
    loadKeepAwakeHoldAfter,
    () => KEEP_AWAKE_HOLD_AFTER_DEFAULT,
  );
  const keepScreen = useSyncExternalStore(
    subscribeKeepAwakeScreen,
    loadKeepAwakeScreen,
    () => false,
  );
  const controller = useRef<ReturnType<
    typeof createKeepAwakeController
  > | null>(null);

  useEffect(() => {
    if (!isTauri()) return;
    const current = createKeepAwakeController(sendNative);
    controller.current = current;
    const release = () => current.release();
    window.addEventListener("pagehide", release);
    window.addEventListener("beforeunload", release);
    return () => {
      window.removeEventListener("pagehide", release);
      window.removeEventListener("beforeunload", release);
      current.release();
      if (controller.current === current) controller.current = null;
    };
  }, []);

  useEffect(() => {
    controller.current?.update(
      enabled,
      sessions,
      keepAwakeHoldAfterMs(holdAfter),
      keepScreen,
    );
  }, [enabled, sessions, holdAfter, keepScreen]);
}

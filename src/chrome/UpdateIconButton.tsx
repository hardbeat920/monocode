import { useEffect, useSyncExternalStore } from "react";
import { ArrowDownCircle, Loader } from "./icons";
import {
  installPendingUpdate,
  probeForUpdate,
  readAppVersion,
  type UpdaterSnapshot,
} from "../lib/updater";

/**
 * Background cadence for update detection: check once on startup, then
 * periodically. Silent unless an update is available — the manual
 * "Check for updates" path lives in Settings.
 */
const BACKGROUND_UPDATE_INTERVAL_MS = 6 * 60 * 60 * 1000;

let snapshot: UpdaterSnapshot = { phase: "idle", currentVersion: "…" };
const listeners = new Set<() => void>();
let started = false;

function emit(next: UpdaterSnapshot) {
  snapshot = next;
  listeners.forEach((listener) => listener());
}

async function runBackgroundCheck() {
  const currentVersion = await readAppVersion();
  try {
    const update = await probeForUpdate();
    if (update) {
      emit({
        phase: "available",
        currentVersion,
        availableVersion: update.version,
      });
      return;
    }
    emit({ phase: "current", currentVersion });
  } catch {
    emit({ phase: "idle", currentVersion });
  }
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot(): UpdaterSnapshot {
  return snapshot;
}

function ensureBackgroundCheck() {
  if (started) return;
  started = true;
  void runBackgroundCheck();
  setInterval(() => {
    void runBackgroundCheck();
  }, BACKGROUND_UPDATE_INTERVAL_MS);
}

export function useBackgroundUpdate(): UpdaterSnapshot {
  useEffect(ensureBackgroundCheck, []);
  return useSyncExternalStore(subscribe, getSnapshot);
}

/**
 * Update affordance for the Settings row. Renders nothing until the
 * background check finds an update — icon button only.
 */
export function UpdateIconButton() {
  const status = useBackgroundUpdate();

  if (status.phase === "downloading") {
    const progress =
      status.progress != null ? ` ${status.progress}%` : "…";
    return (
      <span
        role="status"
        title={`Downloading update${progress}`}
        aria-label={`Downloading update${progress}`}
        className="grid size-8 shrink-0 place-items-center rounded-md text-content/70"
      >
        <Loader className="size-4 animate-spin" aria-hidden />
      </span>
    );
  }

  if (status.phase !== "available" || !status.availableVersion) return null;

  const label = `Update to ${status.availableVersion}`;
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={() => {
        void installPendingUpdate(emit);
      }}
      className="grid size-8 shrink-0 place-items-center rounded-md text-accent hover:bg-content/5"
    >
      <ArrowDownCircle className="size-4" aria-hidden />
    </button>
  );
}

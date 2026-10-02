import {
  isHarnessAvailable,
  probeHarnessAvailability,
} from "../../../integrations/harness/core/availability";
import {
  inspectHarnessBinary,
  updateHarnessCli,
} from "../../../integrations/harness/core/child";
import { refreshHarnessCatalogs } from "../../../integrations/harness/core/registry";
import { HARNESSES, type HarnessId } from "../../sessions/model/session";
import {
  announceHarnessUpdated,
  checkHarnessVersions,
  fetchLatestHarnessVersion,
  isHarnessVersionBehind,
  parseHarnessVersion,
  UPDATABLE_HARNESSES,
  type HarnessUpdate,
  type HarnessVersionCheck,
} from "./harnessUpdates";

export type HarnessUpdateRun =
  | { status: "idle" }
  | { status: "updating" }
  | { status: "updated"; version: string }
  | { status: "failed"; error: string };

export type HarnessUpdateSnapshot = {
  /** Null until this window has checked. */
  checks: HarnessVersionCheck[] | null;
  checking: boolean;
  runs: Partial<Record<HarnessId, HarnessUpdateRun>>;
};

/**
 * One copy per window, read by both the launch toast and Settings, so an
 * update started in one shows its progress in the other.
 */
let snapshot: HarnessUpdateSnapshot = {
  checks: null,
  checking: false,
  runs: {},
};
const listeners = new Set<() => void>();

function setSnapshot(next: Partial<HarnessUpdateSnapshot>) {
  snapshot = { ...snapshot, ...next };
  listeners.forEach((listener) => listener());
}

export function getHarnessUpdateSnapshot(): HarnessUpdateSnapshot {
  return snapshot;
}

export function subscribeHarnessUpdates(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

async function installedVersion(harness: HarnessId): Promise<string> {
  const inspection = await inspectHarnessBinary(harness);
  if (inspection.version) return inspection.version;
  throw new Error(inspection.error ?? "The CLI reported no version.");
}

let inflightCheck: Promise<HarnessVersionCheck[]> | null = null;

/**
 * Checks every installed harness that has a release feed. A call made while
 * a check is running shares its result, unless it forces a fresh probe: the
 * running check may have skipped one, so a forced call runs after it.
 */
export function checkInstalledHarnessVersions(options?: {
  force?: boolean;
}): Promise<HarnessVersionCheck[]> {
  if (inflightCheck && options?.force) {
    return inflightCheck
      .catch(() => undefined)
      .then(() => checkInstalledHarnessVersions(options));
  }
  inflightCheck ??= (async () => {
    setSnapshot({ checking: true });
    try {
      await probeHarnessAvailability(options);
      const checks = await checkHarnessVersions({
        harnesses: HARNESSES.filter(
          (id) => UPDATABLE_HARNESSES.has(id) && isHarnessAvailable(id),
        ),
        installedVersion,
        latestVersion: fetchLatestHarnessVersion,
      });
      setSnapshot({ checks });
      return checks;
    } finally {
      inflightCheck = null;
      setSnapshot({ checking: false });
    }
  })();
  return inflightCheck;
}

const running = new Map<HarnessId, Promise<HarnessUpdateRun>>();

/** A second request for a harness already updating waits on the first run. */
export function runHarnessUpdate(
  update: HarnessUpdate,
): Promise<HarnessUpdateRun> {
  const current = running.get(update.harness);
  if (current) return current;
  setRun(update.harness, { status: "updating" });
  const run = performUpdate(update).then((result) => {
    running.delete(update.harness);
    setRun(update.harness, result);
    if (result.status === "updated") {
      markCurrent(update, result.version);
    }
    return result;
  });
  running.set(update.harness, run);
  return run;
}

function setRun(harness: HarnessId, run: HarnessUpdateRun) {
  setSnapshot({ runs: { ...snapshot.runs, [harness]: run } });
}

function markCurrent(update: HarnessUpdate, version: string) {
  if (!snapshot.checks) return;
  setSnapshot({
    checks: snapshot.checks.map((check) =>
      check.harness === update.harness
        ? {
            harness: update.harness,
            status: "current",
            installed: version,
            latest: update.latest,
          }
        : check,
    ),
  });
}

/**
 * Some updaters exit cleanly without installing anything, so success is the
 * version the CLI reports afterwards, not the exit code. Its models are
 * reloaded before the result says so, so the picker is current by then.
 */
async function performUpdate(update: HarnessUpdate): Promise<HarnessUpdateRun> {
  try {
    await updateHarnessCli(update.harness);
    const after = await inspectHarnessBinary(update.harness);
    const version = parseHarnessVersion(update.harness, after.version ?? "");
    if (
      version &&
      !isHarnessVersionBehind(update.harness, version, update.latest)
    ) {
      await refreshHarnessCatalogs([update.harness], { force: true });
      void announceHarnessUpdated(update.harness).catch(() => undefined);
      return { status: "updated", version };
    }
    return {
      status: "failed",
      error: `Still on ${version ?? update.installed} after updating.`,
    };
  } catch (error) {
    return {
      status: "failed",
      error: error instanceof Error ? error.message : String(error),
    };
  }
}

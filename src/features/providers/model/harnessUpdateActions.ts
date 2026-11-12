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

type FinishedUpdate = {
  installed: string;
  latest: string;
  /** The value of `finishedUpdates` this update set. */
  order: number;
};

/**
 * Counts successful updates in this window. A check that started before an
 * update finished may have run the old binary, so the version the update
 * left wins for that harness.
 */
let finishedUpdates = 0;
const lastUpdates = new Map<HarnessId, FinishedUpdate>();

/**
 * The installed version comes from the update. The release comes from the
 * check, since its feed lookup is the more recent one, unless the check
 * failed.
 */
function withUpdate(
  check: HarnessVersionCheck,
  update: FinishedUpdate,
): HarnessVersionCheck {
  const latest = check.status === "unknown" ? update.latest : check.latest;
  return {
    harness: check.harness,
    status: isHarnessVersionBehind(check.harness, update.installed, latest)
      ? "behind"
      : "current",
    installed: update.installed,
    latest,
  };
}

/**
 * Applies each update to its harness's check. An updated harness the check
 * left out is added, since the updater can replace the binary while the
 * availability probe runs and the probe then reports the CLI as missing.
 */
function withUpdates(
  checks: HarnessVersionCheck[],
  updates: Map<HarnessId, FinishedUpdate>,
): HarnessVersionCheck[] {
  const merged = checks.map((check) => {
    const update = updates.get(check.harness);
    return update ? withUpdate(check, update) : check;
  });
  for (const [harness, update] of updates) {
    if (merged.some((check) => check.harness === harness)) continue;
    merged.push(
      withUpdate({ harness, status: "unknown", error: "Not checked." }, update),
    );
  }
  return merged.sort(
    (a, b) => HARNESSES.indexOf(a.harness) - HARNESSES.indexOf(b.harness),
  );
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
    const startedAfter = finishedUpdates;
    setSnapshot({ checking: true });
    try {
      await probeHarnessAvailability(options);
      const found = await checkHarnessVersions({
        harnesses: HARNESSES.filter(
          (id) => UPDATABLE_HARNESSES.has(id) && isHarnessAvailable(id),
        ),
        installedVersion,
        latestVersion: fetchLatestHarnessVersion,
      });
      const checks = withUpdates(
        found,
        new Map(
          [...lastUpdates].filter(([, update]) => update.order > startedAfter),
        ),
      );
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
      recordUpdate(update, result.version);
    }
    return result;
  });
  running.set(update.harness, run);
  return run;
}

function setRun(harness: HarnessId, run: HarnessUpdateRun) {
  setSnapshot({ runs: { ...snapshot.runs, [harness]: run } });
}

/**
 * A check that finished while the update ran may have found a newer release
 * than the one offered, so the harness can still be behind afterwards.
 */
function recordUpdate(update: HarnessUpdate, version: string) {
  finishedUpdates += 1;
  const finished = {
    installed: version,
    latest: update.latest,
    order: finishedUpdates,
  };
  lastUpdates.set(update.harness, finished);
  if (!snapshot.checks) return;
  setSnapshot({
    checks: withUpdates(snapshot.checks, new Map([[update.harness, finished]])),
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

import { invoke } from "@tauri-apps/api/core";
import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { HarnessId } from "../../sessions/model/session";
import {
  compareSemver,
  parseOpenCodeVersion,
} from "../../../integrations/harness/providers/opencode/opencodeProtocol";

/**
 * Harnesses with an npm version feed and a self-updater MonoCode can run.
 */
export const UPDATABLE_HARNESSES: ReadonlySet<HarnessId> = new Set([
  "claude",
  "codex",
  "cursor",
  "grok",
  "opencode",
  "pi",
  "omp",
  "fx",
]);

export type HarnessUpdate = {
  harness: HarnessId;
  installed: string;
  latest: string;
};

export type HarnessVersionCheck =
  | (HarnessUpdate & { status: "current" })
  | (HarnessUpdate & { status: "behind" })
  | { harness: HarnessId; status: "unknown"; error: string };

export type HarnessUpdateDeps = {
  /** Installed harnesses to check. */
  harnesses: HarnessId[];
  installedVersion: (harness: HarnessId) => Promise<string | undefined>;
  latestVersion: (harness: HarnessId) => Promise<string>;
};

/** Every window keeps its own model catalog, so each has to hear about it. */
const HARNESS_UPDATED_EVENT = "harness-updated";
const updateEventSource = crypto.randomUUID();

type HarnessUpdatedEvent = {
  harness: HarnessId;
  source: string;
};

export function announceHarnessUpdated(harness: HarnessId): Promise<void> {
  return emit(HARNESS_UPDATED_EVENT, { harness, source: updateEventSource });
}

export function onHarnessUpdated(
  handler: (harness: HarnessId) => void,
): Promise<UnlistenFn> {
  return listen<HarnessUpdatedEvent>(HARNESS_UPDATED_EVENT, (event) => {
    // The sender awaited its local refresh before announcing the update.
    if (event.payload.source === updateEventSource) return;
    handler(event.payload.harness);
  });
}

export function claimLaunchHarnessUpdateCheck(): Promise<boolean> {
  return invoke<boolean>("harness_update_check_claim");
}

export function fetchLatestHarnessVersion(harness: HarnessId): Promise<string> {
  return invoke<string>("harness_latest_version", { provider: harness });
}

/** Cursor names a build by date and commit, such as `2026.09.28-64d2043`. */
const CURSOR_BUILD = /\d{4}\.\d{2}\.\d{2}-[0-9a-f]+/;

/**
 * The version to compare and display. Cursor keeps its full build, because
 * two builds can share a date.
 */
export function parseHarnessVersion(
  harness: HarnessId,
  output: string,
): string | null {
  if (harness === "cursor") {
    const build = output.match(CURSOR_BUILD)?.[0];
    if (build) return build;
  }
  return parseOpenCodeVersion(output);
}

/**
 * True when `installed` is older than `latest`. A Cursor build from the same
 * day as the feed but with another commit is behind, since the feed names
 * the newest build. Commit hashes have no order, so this is the only way to
 * tell.
 */
export function isHarnessVersionBehind(
  harness: HarnessId,
  installed: string,
  latest: string,
): boolean {
  const order = compareSemver(latest, installed);
  if (order !== 0) return order > 0;
  return harness === "cursor" && installed !== latest;
}

/**
 * Compares each harness with its newest release. A failed lookup becomes an
 * "unknown" entry for that harness instead of failing the whole check.
 * Installed builds newer than the feed, such as a dev channel, count as
 * current.
 */
export async function checkHarnessVersions({
  harnesses,
  installedVersion,
  latestVersion,
}: HarnessUpdateDeps): Promise<HarnessVersionCheck[]> {
  return Promise.all(
    harnesses
      .filter((harness) => UPDATABLE_HARNESSES.has(harness))
      .map(async (harness): Promise<HarnessVersionCheck> => {
        try {
          const [installedOutput, latestOutput] = await Promise.all([
            installedVersion(harness),
            latestVersion(harness),
          ]);
          const installed = parseHarnessVersion(harness, installedOutput ?? "");
          if (!installed) {
            return {
              harness,
              status: "unknown",
              error: "The CLI reported no version.",
            };
          }
          const latest = parseHarnessVersion(harness, latestOutput);
          if (!latest) {
            return {
              harness,
              status: "unknown",
              error: "The release feed returned no version.",
            };
          }
          const status = isHarnessVersionBehind(harness, installed, latest)
            ? "behind"
            : "current";
          return { harness, status, installed, latest };
        } catch (error) {
          return {
            harness,
            status: "unknown",
            error: error instanceof Error ? error.message : String(error),
          };
        }
      }),
  );
}

/** Only the harnesses behind their newest release. */
export function pendingHarnessUpdates(
  checks: HarnessVersionCheck[],
): HarnessUpdate[] {
  return checks.flatMap((check) =>
    check.status === "behind"
      ? [
          {
            harness: check.harness,
            installed: check.installed,
            latest: check.latest,
          },
        ]
      : [],
  );
}

/**
 * This runs unprompted at launch, so a failed lookup drops that harness
 * silently. Nothing is remembered between launches: a harness still behind
 * is offered again, at whatever release is newest by then.
 */
export async function findHarnessUpdates(
  deps: HarnessUpdateDeps,
): Promise<HarnessUpdate[]> {
  return pendingHarnessUpdates(await checkHarnessVersions(deps));
}

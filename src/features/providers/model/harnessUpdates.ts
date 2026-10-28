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
  "opencode",
  "pi",
]);

export type HarnessUpdate = {
  harness: HarnessId;
  installed: string;
  latest: string;
};

export type HarnessUpdateDeps = {
  /** Installed harnesses the user has not hidden. */
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

/**
 * A failed lookup, offline or otherwise, drops that harness silently: this
 * runs unprompted at launch and must never surface an error of its own.
 * Nothing is remembered between launches: a harness still behind is offered
 * again, at whatever release is newest by then.
 */
export async function findHarnessUpdates({
  harnesses,
  installedVersion,
  latestVersion,
}: HarnessUpdateDeps): Promise<HarnessUpdate[]> {
  const results = await Promise.all(
    harnesses.map(async (harness): Promise<HarnessUpdate | null> => {
      if (!UPDATABLE_HARNESSES.has(harness)) return null;
      try {
        const [installedOutput, latestOutput] = await Promise.all([
          installedVersion(harness),
          latestVersion(harness),
        ]);
        const installed = parseOpenCodeVersion(installedOutput ?? "");
        const latest = parseOpenCodeVersion(latestOutput);
        if (!installed || !latest) return null;
        if (compareSemver(latest, installed) <= 0) return null;
        return { harness, installed, latest };
      } catch {
        return null;
      }
    }),
  );
  return results.filter((update): update is HarnessUpdate => update !== null);
}

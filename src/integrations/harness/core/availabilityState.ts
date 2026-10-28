import type { HarnessId } from "../../../features/sessions/model/session";

/**
 * The probed installer state for every harness. Kept free of the binary
 * resolvers so the model layer can consult it without loading the whole
 * harness integration graph.
 */
export type HarnessAvailability = Record<HarnessId, boolean>;

let availability: HarnessAvailability = {
  claude: false,
  codex: false,
  cursor: false,
  grok: false,
  opencode: false,
  pi: false,
  omp: false,
  fx: false,
  hermes: false,
  antigravity: false,
};
let version = 0;
let probedAt = 0;
const listeners = new Set<() => void>();

export function emitHarnessAvailability() {
  version += 1;
  for (const listener of listeners) listener();
}

export function subscribeHarnessAvailability(
  onStoreChange: () => void,
): () => void {
  listeners.add(onStoreChange);
  return () => {
    listeners.delete(onStoreChange);
  };
}

export function getHarnessAvailabilitySnapshot(): number {
  return version;
}

export function hasProbedHarnessAvailability(): boolean {
  return probedAt > 0;
}

export function isHarnessAvailable(id: HarnessId): boolean {
  return availability[id];
}

export function setHarnessAvailability(next: HarnessAvailability): void {
  availability = next;
}

/** When the probe last finished, or `0` before the first probe. */
export function harnessAvailabilityProbedAt(): number {
  return probedAt;
}

export function markHarnessAvailabilityProbed(): void {
  probedAt = Date.now();
}

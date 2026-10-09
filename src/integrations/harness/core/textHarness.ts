import type { HarnessId } from "../../../features/sessions/model/session";
import { isProviderHidden } from "../../../features/sessions/model/projectProviders";
import { isHarnessAvailable } from "./availability";
import { isPickerProviderVisible } from "../../../features/sessions/model/models";
import type { PrContent } from "../../../features/source-control/model/gitText";
import {
  generateHarnessCommitMessage,
  generateHarnessPrContent,
  warmupHarnessText,
} from "./registry";

const TEXT_HARNESSES: HarnessId[] = [
  "claude",
  "cursor",
  "codex",
  "grok",
  "opencode",
];

export const NO_TEXT_HARNESS_MESSAGE =
  "No text provider is available. Install one or turn on Show in picker in Settings.";

/**
 * Pick the harness used for titles, commit messages, PR text, and branch
 * names. `preferred` is the active session's provider and wins when installed.
 * Fallbacks skip providers hidden globally or in this project.
 */
export function pickTextHarness(
  preferred?: HarnessId,
  project?: string,
): HarnessId | null {
  const ordered =
    preferred && TEXT_HARNESSES.includes(preferred)
      ? [preferred, ...TEXT_HARNESSES.filter((id) => id !== preferred)]
      : TEXT_HARNESSES;
  for (const id of ordered) {
    if (!isHarnessAvailable(id)) continue;
    if (
      id === preferred ||
      (isPickerProviderVisible(id) && !isProviderHidden(project, id))
    ) {
      return id;
    }
  }
  return null;
}

export function warmupText(
  cwd: string,
  preferred?: HarnessId,
  project = cwd,
): Promise<void> {
  const harness = pickTextHarness(preferred, project);
  return harness ? warmupHarnessText(harness, cwd) : Promise.resolve();
}

export function generateCommitMessage(
  cwd: string,
  preferred?: HarnessId,
  signal?: AbortSignal,
  project = cwd,
): Promise<string> {
  const harness = pickTextHarness(preferred, project);
  return harness
    ? generateHarnessCommitMessage(harness, cwd, signal)
    : Promise.reject(new Error(NO_TEXT_HARNESS_MESSAGE));
}

export function generatePrContent(
  cwd: string,
  preferred?: HarnessId,
  project = cwd,
): Promise<(PrContent & { base: string; head: string }) | null> {
  const harness = pickTextHarness(preferred, project);
  return harness
    ? generateHarnessPrContent(harness, cwd)
    : Promise.reject(new Error(NO_TEXT_HARNESS_MESSAGE));
}

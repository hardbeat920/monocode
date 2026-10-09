import { HARNESSES } from "../../sessions/model/session";
import { isProviderHidden } from "../../sessions/model/projectProviders";
import {
  isPickerProviderVisible,
  modelsFor,
} from "../../sessions/model/models";
import {
  isHarnessAvailable,
  probeHarnessAvailability,
} from "../../../integrations/harness/core/availability";
import { refreshHarnessCatalogs } from "../../../integrations/harness/core/registry";
import { validateOrchestrationSettings } from "./orchestrationPlan";

/** Discover worker choices only when the user sends an orchestration request. */
export async function discoverOrchestrationSettings(project?: string) {
  await probeHarnessAvailability();
  // Worker sessions obey the same global and project visibility as the picker.
  const isEligible = (id: (typeof HARNESSES)[number]) =>
    isHarnessAvailable(id) &&
    isPickerProviderVisible(id) &&
    !isProviderHidden(project, id);
  const installed = HARNESSES.filter(isEligible);
  await refreshHarnessCatalogs(installed);
  return validateOrchestrationSettings({
    maxWorkers: 2,
    choices: installed.filter(isEligible).flatMap((harness) =>
      modelsFor(harness).map(({ id, name }) => ({
        harness,
        model: id,
        name,
      })),
    ),
  });
}

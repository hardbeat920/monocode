import { HARNESSES } from "../../sessions/model/session";
import { modelsFor } from "../../sessions/model/models";
import {
  isHarnessAvailable,
  probeHarnessAvailability,
} from "../../../integrations/harness/core/availability";
import { refreshHarnessCatalogs } from "../../../integrations/harness/core/registry";
import { validateOrchestrationSettings } from "./orchestrationPlan";

/** Discover worker choices only when the user sends an orchestration request. */
export async function discoverOrchestrationSettings(cwd?: string) {
  await probeHarnessAvailability({ cwd });
  const installed = HARNESSES.filter(isHarnessAvailable);
  await refreshHarnessCatalogs(installed, { cwd });
  return validateOrchestrationSettings({
    maxWorkers: 2,
    choices: installed
      .filter(isHarnessAvailable)
      .flatMap((harness) =>
        modelsFor(harness).map(({ id, name }) => ({
          harness,
          model: id,
          name,
        })),
      ),
  });
}

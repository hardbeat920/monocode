import { useSyncExternalStore } from "react";
import { pickTextHarness } from "../../../integrations/harness";
import {
  getHarnessAvailabilitySnapshot,
  subscribeHarnessAvailability,
} from "../../../integrations/harness/core/availability";
import {
  getPickerVisibilitySnapshot,
  subscribePickerVisibility,
} from "../../sessions/model/models";
import {
  projectProvidersRevision,
  subscribeProjectProviders,
} from "../../sessions/model/projectProviders";
import type { HarnessId } from "../../sessions/model/session";

export function useTextHarness(
  preferred?: HarnessId,
  project?: string,
): HarnessId | null {
  useSyncExternalStore(
    subscribeHarnessAvailability,
    getHarnessAvailabilitySnapshot,
  );
  useSyncExternalStore(subscribePickerVisibility, getPickerVisibilitySnapshot);
  useSyncExternalStore(subscribeProjectProviders, projectProvidersRevision);
  return pickTextHarness(preferred, project);
}

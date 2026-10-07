import { useSyncExternalStore } from "react";
import {
  loadRawToolCalls,
  subscribeRawToolCalls,
} from "../../settings/model/appearance";

/** Whether readable tool rows also open onto the raw command. */
export function useRawToolCalls(): boolean {
  return useSyncExternalStore(
    subscribeRawToolCalls,
    loadRawToolCalls,
    loadRawToolCalls,
  );
}

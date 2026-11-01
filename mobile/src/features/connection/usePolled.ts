import { useIsFocused } from "expo-router";
import { useEffect, useSyncExternalStore } from "react";
import type { PolledResource } from "./polledResource";

/**
 * What a shared poller last read, kept polling only while `enabled` and this
 * screen is focused (a screen covered by another or in a hidden tab doesn't
 * read, and the app in the background doesn't either).
 */
export function usePolled<T>(
  resource: PolledResource<T>,
  fetch: () => Promise<T>,
  enabled: boolean,
) {
  const focused = useIsFocused();
  const polling = enabled && focused;
  useEffect(() => (polling ? resource.hold(fetch) : undefined), [resource, fetch, polling]);
  return useSyncExternalStore(resource.subscribe, resource.getSnapshot);
}

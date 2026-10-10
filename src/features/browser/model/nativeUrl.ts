import { isNativeTabReady } from "./nativeTabs";
import {
  dockOfTab,
  getBrowserState,
  liveBrowserTabIds,
  loadGeneration,
  nativeUrlEpoch,
  noteNativeUrl,
  patchBrowserTab,
  urlRevision,
} from "./browserStore";
import { isBrowsableUrl } from "./browserUrl";
import { isNavigating } from "./browserNavigation";

function tabBrowser(id: string) {
  return dockOfTab(getBrowserState(), id)?.pane.files.find(
    (file) => file.id === id,
  )?.browser;
}

/** Tabs whose native page is running now. Closed and suspended tabs have none. */
export function trackedBrowserTabIds(): string[] {
  const state = getBrowserState();
  return [...liveBrowserTabIds(state)].filter(isNativeTabReady);
}

/**
 * Pull one tab's URL from its native page and apply it when it moved without a
 * load (pushState, replaceState, hash change). Only the URL changes; loading
 * belongs to load events. A reply is dropped if the tab went away or was
 * suspended, a load started, or anything wrote the URL while it was in flight.
 */
export async function syncNativeUrl(
  id: string,
  readUrl: (id: string) => Promise<string>,
): Promise<boolean> {
  const before = tabBrowser(id);
  if (!before || before.loading || isNavigating(id) || !isNativeTabReady(id)) {
    return false;
  }
  const generation = loadGeneration(id);
  const epoch = nativeUrlEpoch(id);
  const revision = urlRevision(id);
  let url: string;
  try {
    url = await readUrl(id);
  } catch {
    return false;
  }
  const tab = tabBrowser(id);
  if (
    !tab ||
    tab.loading ||
    isNavigating(id) ||
    !isNativeTabReady(id) ||
    loadGeneration(id) !== generation ||
    nativeUrlEpoch(id) !== epoch ||
    urlRevision(id) !== revision ||
    url === tab.url ||
    !isBrowsableUrl(url)
  ) {
    return false;
  }
  noteNativeUrl(id);
  patchBrowserTab(id, { url });
  return true;
}

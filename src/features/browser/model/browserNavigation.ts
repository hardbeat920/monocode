import { navigateBrowser } from "../../../platform/tauri/browser";
import {
  dockOfTab,
  getBrowserState,
  loadGeneration,
  nativeUrlEpoch,
  patchBrowserTab,
} from "./browserStore";

interface Inflight {
  latest: number;
  count: number;
  /** Rollback baseline: the URL before the unsettled requests, kept current. */
  url?: string;
  /** Native URL epoch the baseline was taken at. */
  epoch: number;
  /** Highest request number accepted so far. */
  accepted: number;
  /** Request whose rejection last restored the baseline, until superseded. */
  restoredBy?: number;
  restoredEpoch?: number;
}

const inflight = new Map<string, Inflight>();

/** Whether a navigation request for the tab is still unsettled. */
export function isNavigating(id: string): boolean {
  return inflight.has(id);
}

/** Native load events own loading: hash changes and rejected loads may emit none. */
export async function navigateBrowserTab(
  id: string,
  url: string,
): Promise<void> {
  const browser = () =>
    dockOfTab(getBrowserState(), id)?.pane.files.find((file) => file.id === id)
      ?.browser;
  let entry = inflight.get(id);
  if (!entry) {
    entry = {
      latest: 0,
      count: 0,
      url: browser()?.url,
      epoch: nativeUrlEpoch(id),
      accepted: 0,
    };
    inflight.set(id, entry);
  } else if (entry.epoch !== nativeUrlEpoch(id)) {
    // Native load events reported a URL since the baseline; it is authoritative.
    entry.url = browser()?.url;
    entry.epoch = nativeUrlEpoch(id);
    entry.restoredBy = undefined;
  }
  const state = entry;
  state.count += 1;
  const request = ++state.latest;
  const generation = loadGeneration(id);
  const epoch = nativeUrlEpoch(id);
  patchBrowserTab(id, { url, error: undefined });
  try {
    await navigateBrowser(id, url);
    // Accepted, so a later rejection must not roll back past it.
    if (state.epoch === epoch && request > state.accepted) {
      state.accepted = request;
      state.url = url;
      // A newer request already rejected and restored the older url; this
      // acceptance is the latest word, so show it. Keep the restore marker so
      // a still-newer acceptance can also update the display.
      if (
        state.restoredBy !== undefined &&
        state.restoredBy > request &&
        state.latest === state.restoredBy &&
        state.restoredEpoch === epoch &&
        nativeUrlEpoch(id) === epoch &&
        browser()
      ) {
        patchBrowserTab(id, { url });
      }
    }
  } catch (reason) {
    const current =
      state.latest === request &&
      nativeUrlEpoch(id) === epoch &&
      !!dockOfTab(getBrowserState(), id);
    if (current) {
      state.restoredBy = request;
      state.restoredEpoch = epoch;
      patchBrowserTab(id, {
        error: String(reason),
        // Only the optimistic loading from opening a tab is ours to clear.
        ...(generation === 0 ? { loading: false } : {}),
        ...(state.url ? { url: state.url } : {}),
      });
    }
    throw reason;
  } finally {
    state.count -= 1;
    if (state.count === 0 && inflight.get(id) === state) inflight.delete(id);
  }
}

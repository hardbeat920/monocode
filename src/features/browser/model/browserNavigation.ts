import {
  navigateBrowser,
  type BrowserNavigation,
} from "../../../platform/tauri/browser";
import {
  dockOfTab,
  getBrowserState,
  loadGeneration,
  nativeUrlEpoch,
  patchBrowserTab,
  subscribeBrowser,
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

interface Settling {
  /** Load generation when the request was dispatched natively, not queued. */
  generation: number;
  /** Order the request was made in, across every request of every tab. */
  order: number;
  /** When to stop waiting for a load that may never report itself. */
  expires: number;
}

/**
 * A load that fails before it commits or is denied can report no event, and so
 * can a navigation whose outcome is unknown, so waiting for a load start is
 * bounded. A document load is given long enough that a slow start does not let
 * polling roll the URL back; an unknown outcome was never promised a load.
 */
export const DOCUMENT_SETTLE_MS = 30_000;
export const UNKNOWN_SETTLE_MS = 3_000;

/**
 * How long a navigation waits, before it is dispatched, for an earlier one of
 * its tab to start loading. The webview accepts a navigation before its load
 * starts, so without the wait the earlier load would count as the later one's.
 */
export const DISPATCH_GATE_MS = 2_000;

let requestOrder = 0;

/**
 * Navigations the webview accepted that have not started loading. The IPC
 * resolves before the load event, so until a load starts the native URL may
 * still be the old document's. An entry ends when a load starts or, as the
 * recovery for loads that never report one, when it expires.
 *
 * Applied navigations never get an entry: the page settled them before the
 * reply, so a read after the reply already shows their outcome. One also
 * supersedes the older entries: the page navigated away from
 * whatever they were waiting for, and a load that does start anyway reports
 * itself. Only handlers that run synchronously are accounted for; a page that
 * changes its URL later is seen by polling like any other same-document change.
 */
const settling = new Map<string, Set<Settling>>();

function clearSettling(id: string) {
  settling.delete(id);
}

/** Drop entries of tabs that left the store, and of tabs whose load started. */
function pruneSettling(id?: string) {
  const live = getBrowserState();
  for (const key of [...settling.keys()]) {
    if (id !== undefined && key !== id) {
      if (!dockOfTab(live, key)) clearSettling(key);
      continue;
    }
    const entries = settling.get(key);
    if (!entries || !dockOfTab(live, key)) {
      clearSettling(key);
      continue;
    }
    const now = Date.now();
    for (const entry of entries) {
      if (loadGeneration(key) !== entry.generation || now >= entry.expires) {
        entries.delete(entry);
      }
    }
    if (entries.size === 0) clearSettling(key);
  }
}

/** Whether a navigation request for the tab is unsettled or awaiting its load. */
export function isNavigating(id: string): boolean {
  if (inflight.has(id)) return true;
  pruneSettling(id);
  return settling.has(id);
}

/** A same-document navigation completed: older waiting entries are superseded. */
function supersede(id: string, order: number) {
  const entries = settling.get(id);
  if (!entries) return;
  for (const entry of entries) {
    if (entry.order < order) entries.delete(entry);
  }
  if (entries.size === 0) clearSettling(id);
}

/** Whether an earlier request of the tab still waits for its load to start. */
function hasEarlierSettling(id: string, order: number): boolean {
  pruneSettling(id);
  return [...(settling.get(id) ?? [])].some((entry) => entry.order < order);
}

/**
 * Resolve once no earlier request of the tab waits for its load to start, or
 * after `DISPATCH_GATE_MS`. Resolves with whether the earlier ones cleared.
 */
function earlierLoadsStarted(id: string, order: number): Promise<boolean> {
  return new Promise((resolve) => {
    const finish = (cleared: boolean) => {
      unsubscribe();
      clearTimeout(timer);
      resolve(cleared);
    };
    // Load starts are reported through the store, with the loading flag.
    const unsubscribe = subscribeBrowser(() => {
      if (!hasEarlierSettling(id, order)) finish(true);
    });
    const timer = setTimeout(
      () => finish(!hasEarlierSettling(id, order)),
      DISPATCH_GATE_MS,
    );
  });
}

export type TabNavigation = {
  outcome: BrowserNavigation["outcome"];
  /**
   * The tab's load generation when this navigation was dispatched natively. A
   * load counted after it may belong to this navigation; one before it cannot.
   */
  since: number;
  /**
   * An earlier navigation of the tab had still not started loading when this
   * one was dispatched, so a load counted after `since` may be that one's.
   */
  overlapped: boolean;
};

/**
 * Native load events own loading: applied and rejected navigations emit none.
 * Resolves with what the native side knows of the outcome.
 */
export async function navigateBrowserTab(
  id: string,
  url: string,
): Promise<TabNavigation> {
  const browser = () =>
    dockOfTab(getBrowserState(), id)?.pane.files.find((file) => file.id === id)
      ?.browser;
  pruneSettling();
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
  // Whether this is still the newest request, on the epoch it was made in.
  const current = () =>
    state.latest === request &&
    nativeUrlEpoch(id) === epoch &&
    !!dockOfTab(getBrowserState(), id);
  state.count += 1;
  const request = ++state.latest;
  const order = ++requestOrder;
  // Read again when the request's turn comes: earlier requests of the tab may
  // start loads while this one waits in the queue, and those are not its own.
  let generation = loadGeneration(id);
  let overlapped = false;
  const epoch = nativeUrlEpoch(id);
  patchBrowserTab(id, { url, error: undefined });
  try {
    const reply = await navigateBrowser(id, url, () => {
      const take = (cleared: boolean) => {
        generation = loadGeneration(id);
        overlapped = !cleared;
      };
      if (!hasEarlierSettling(id, order)) return take(true);
      return earlierLoadsStarted(id, order).then(take);
    });
    // An unreported outcome is a document navigation: its load events decide.
    const outcome = reply?.outcome ?? "document";
    if (outcome === "applied") {
      supersede(id, order);
    } else if (
      (outcome === "document" || outcome === "unknown") &&
      dockOfTab(getBrowserState(), id) &&
      // A load that started since the request already took over from us.
      loadGeneration(id) === generation
    ) {
      const entries = settling.get(id) ?? new Set<Settling>();
      entries.add({
        generation,
        order,
        expires:
          Date.now() +
          (outcome === "document" ? DOCUMENT_SETTLE_MS : UNKNOWN_SETTLE_MS),
      });
      settling.set(id, entries);
    }
    // Where the page's handlers left an applied navigation, else the target.
    const landed = (outcome === "applied" && reply?.url) || url;
    if (landed !== url && current()) patchBrowserTab(id, { url: landed });
    // Accepted, so a later rejection must not roll back past it.
    if (state.epoch === epoch && request > state.accepted) {
      state.accepted = request;
      state.url = landed;
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
        patchBrowserTab(id, { url: landed });
      }
    }
    return { outcome, since: generation, overlapped };
  } catch (reason) {
    if (current()) {
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

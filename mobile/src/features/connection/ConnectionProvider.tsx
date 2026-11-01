import {
  createContext,
  use,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import { AppState, Platform } from "react-native";
import Constants from "expo-constants";
import * as SecureStore from "expo-secure-store";
import Storage from "expo-sqlite/kv-store";
import type {
  Project,
  Mono,
  SessionSummary,
  Message,
  QuestionPrompt,
  Turn,
} from "@/features/workspace/types";
import {
  nativeCall,
  parseInvitation,
  request,
  type Pairing,
  type Desktop,
  type Identity,
} from "./nativeLink";
import { backoff, onAppActive, pause, waitForActive } from "./appActivity";
import { deepEqual } from "./deepEqual";
import { mergeCatalog } from "./catalogMerge";
import { normalizeSelection, type MobileSelection } from "./selection";

export type Catalog = {
  projects: Project[];
  monos: Mono[];
  sessions: SessionSummary[];
  addresses?: Desktop["addresses"];
  /** The desktop answers Mono profiles, editing, reset, delete and create;
   *  the older desktop app sends no such flag. */
  monoTools?: boolean;
  /** The desktop sends this phone push notifications once it registers;
   *  the older desktop app doesn't, and the phone posts its own. */
  push?: boolean;
  /** The desktop reads and saves a Mono's soul and memory for the phone. */
  monoFiles?: boolean;
};
const EMPTY: Catalog = { projects: [], monos: [], sessions: [] };
const PAIRING = "monocode.mobile.pairing";
const IDENTITY = "monocode.mobile.identity";
const key = (fingerprint: string, kind: string) =>
  `monocode.mobile.${fingerprint}.${kind}`;
/** How often the workspace is read while the app is in the foreground. */
export const POLL_MS = 5000;
/** After failed reads the wait doubles, up to this. */
const MAX_POLL_MS = 30000;
/** The catalog is saved for the next launch at most this often, and when the app leaves the foreground. */
export const CATALOG_SAVE_MS = 30000;

type Actions = {
  pair: (text: string) => Promise<void>;
  refresh: () => Promise<void>;
  select: (selection: MobileSelection) => Promise<void>;
  disconnect: () => Promise<void>;
  command: <T>(method: string, params?: object, id?: string) => Promise<T>;
};
type State = {
  restoring: boolean;
  pairing: Pairing | null;
  selection: MobileSelection | null;
  connected: boolean;
  working: boolean;
  error: string | null;
};
type Connection = State & Actions & { catalog: Catalog };

/** The catalog outside React state, so a screen reads just the part it needs. */
class CatalogStore {
  private value: Catalog = EMPTY;
  private listeners = new Set<() => void>();
  get = () => this.value;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  set(next: Catalog) {
    if (next === this.value) return;
    this.value = next;
    for (const listener of [...this.listeners]) listener();
  }
}
// Actions never change for the life of the app, so what reads only them never re-renders.
const ActionsContext = createContext<Actions | null>(null);
const StateContext = createContext<State | null>(null);
const CatalogContext = createContext<CatalogStore | null>(null);
export function ConnectionProvider({ children }: { children: ReactNode }) {
  const [restoring, setRestoring] = useState(true);
  const [pairing, setPairing] = useState<Pairing | null>(null);
  const [store] = useState(() => new CatalogStore());
  const [selection, setSelection] = useState<MobileSelection | null>(null);
  const [connected, setConnected] = useState(false);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const active = useRef<Pairing | null>(null);
  const refreshing = useRef(false);
  const failures = useRef(0);
  // The catalog saved for the next launch is written lazily, not on every change.
  const unsaved = useRef<{ fingerprint: string; catalog: Catalog } | null>(null);
  const savedAt = useRef(0);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const flushCatalog = useCallback(() => {
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = null;
    const pending = unsaved.current;
    unsaved.current = null;
    if (!pending) return;
    savedAt.current = Date.now();
    void Storage.setItem(key(pending.fingerprint, "catalog"), JSON.stringify(pending.catalog));
  }, []);
  const saveCatalog = useCallback(
    (fingerprint: string, catalog: Catalog) => {
      unsaved.current = { fingerprint, catalog };
      if (saveTimer.current) return;
      const wait = Math.max(0, savedAt.current + CATALOG_SAVE_MS - Date.now());
      saveTimer.current = setTimeout(flushCatalog, wait);
    },
    [flushCatalog],
  );
  const pairingNow = useRef(false);
  const generation = useRef(0);
  useEffect(() => {
    let disposed = false;
    void (async () => {
      try {
        const [desktop, identity] = await Promise.all([
          Storage.getItem(PAIRING),
          SecureStore.getItemAsync(IDENTITY),
        ]);
        if (disposed || !desktop || !identity) return;
        const value: Pairing = {
          desktop: JSON.parse(desktop),
          identity: JSON.parse(identity),
        };
        const [chosen, saved] = await Promise.all([
          Storage.getItem(key(value.desktop.fingerprint, "selection")),
          Storage.getItem(key(value.desktop.fingerprint, "catalog")),
        ]);
        if (disposed) return;
        active.current = value;
        setPairing(value);
        setSelection(chosen ? normalizeSelection(JSON.parse(chosen)) : null);
        if (saved) store.set(JSON.parse(saved));
      } catch {
        if (!disposed)
          setError("Could not restore the desktop connection. Connect again.");
      } finally {
        if (!disposed) setRestoring(false);
      }
    })();
    return () => {
      disposed = true;
    };
  }, [store]);
  const refresh = useCallback(async () => {
    const value = active.current;
    if (!value || refreshing.current) return;
    const version = generation.current;
    refreshing.current = true;
    try {
      const next = await request<Catalog>(value, "workspace.list");
      if (active.current !== value || version !== generation.current) return;
      if (
        !Array.isArray(next.projects) ||
        !Array.isArray(next.monos) ||
        !Array.isArray(next.sessions)
      )
        throw new Error("Desktop sent an invalid workspace");
      // The addresses only matter once they change.
      if (next.addresses?.length && !deepEqual(value.desktop.addresses, next.addresses)) {
        value.desktop.addresses = next.addresses;
        await Storage.setItem(PAIRING, JSON.stringify(value.desktop));
        if (active.current !== value) return;
      }
      // The poll repeats every few seconds; only a changed workspace is
      // re-rendered, and it is saved a little later.
      const { addresses: _, ...workspace } = next;
      const merged = mergeCatalog(store.get(), workspace);
      if (merged !== store.get()) {
        store.set(merged);
        saveCatalog(value.desktop.fingerprint, merged);
      }
      failures.current = 0;
      setConnected(true);
      setError(null);
    } catch (reason) {
      if (active.current === value) {
        failures.current++;
        setConnected(false);
        setError(reason instanceof Error ? reason.message : String(reason));
      }
    } finally {
      refreshing.current = false;
    }
  }, [store, saveCatalog]);
  useEffect(() => {
    if (!pairing) return;
    const stop = new AbortController();
    void (async () => {
      while (!stop.signal.aborted) {
        // In the background there is nothing to read for; wait to be shown again.
        await waitForActive(stop.signal);
        if (stop.signal.aborted) break;
        await refresh();
        await pause(
          failures.current > 0
            ? backoff(POLL_MS, failures.current - 1, MAX_POLL_MS)
            : POLL_MS,
          stop.signal,
        );
      }
    })();
    const stopActive = onAppActive(() => void refresh());
    // Leaving the foreground may end the app, so save what is waiting.
    const subscription = AppState.addEventListener("change", (state) => {
      if (state !== "active") flushCatalog();
    });
    return () => {
      stop.abort();
      stopActive();
      subscription.remove();
    };
  }, [pairing, refresh, flushCatalog]);
  const pair = useCallback(async (text: string) => {
    if (pairingNow.current) return;
    pairingNow.current = true;
    setWorking(true);
    setError(null);
    try {
      const invitation = parseInvitation(text);
      const existing = await SecureStore.getItemAsync(IDENTITY);
      const identity = existing
        ? (JSON.parse(existing) as Identity)
        : await nativeCall<Identity>({ operation: "identity" });
      await SecureStore.setItemAsync(IDENTITY, JSON.stringify(identity), {
        keychainAccessible: SecureStore.WHEN_UNLOCKED_THIS_DEVICE_ONLY,
      });
      const fingerprint = await nativeCall<string>({
        operation: "fingerprint",
        identity: invitation.identity,
      });
      const proof = await nativeCall<string>({
        operation: "proof",
        identity,
        invitation: fingerprint,
      });
      const desktop = await nativeCall<Desktop>({
        operation: "exchange",
        identity: invitation.identity,
        pin: invitation.fingerprint,
        addresses: invitation.addresses,
        request: {
          version: 1,
          id: "enroll",
          method: "device.enroll",
          params: {
            certificate: identity.certificate,
            proof,
            name: deviceName(),
          },
        },
      });
      const value = { desktop, identity };
      await Storage.setItem(PAIRING, JSON.stringify(desktop));
      const chosen = await Storage.getItem(
        key(desktop.fingerprint, "selection"),
      );
      generation.current++;
      active.current = value;
      setPairing(value);
      store.set(EMPTY);
      unsaved.current = null;
      failures.current = 0;
      setSelection(chosen ? normalizeSelection(JSON.parse(chosen)) : null);
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      pairingNow.current = false;
      setWorking(false);
    }
  }, [refresh, store]);
  const select = useCallback(async (value: MobileSelection) => {
    const pairing = active.current;
    if (!pairing) return;
    await Storage.setItem(
      key(pairing.desktop.fingerprint, "selection"),
      JSON.stringify(value),
    );
    if (active.current === pairing) setSelection(value);
  }, []);
  const disconnect = useCallback(async () => {
    const value = active.current;
    generation.current++;
    active.current = null;
    setPairing(null);
    setConnected(false);
    store.set(EMPTY);
    unsaved.current = null;
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = null;
    setSelection(null);
    setError(null);
    await Storage.removeItem(PAIRING);
    if (value) {
      void request(value, "device.leave").catch(() => {});
      await Storage.removeItem(key(value.desktop.fingerprint, "catalog"));
    }
    // Keep this installation's identity, so a new invitation can recover cleanly.
  }, [store]);
  const command = useCallback(
    async <T,>(
      method: string,
      params: object = {},
      id?: string,
    ): Promise<T> => {
      const value = active.current;
      if (!value) throw new Error("Connect your desktop first");
      return request<T>(value, method, params, id);
    },
    [],
  );
  const actions = useMemo(
    () => ({ pair, refresh, select, disconnect, command }),
    [pair, refresh, select, disconnect, command],
  );
  const state = useMemo(
    () => ({ restoring, pairing, selection, connected, working, error }),
    [restoring, pairing, selection, connected, working, error],
  );
  return (
    <ActionsContext value={actions}>
      <StateContext value={state}>
        <CatalogContext value={store}>{children}</CatalogContext>
      </StateContext>
    </ActionsContext>
  );
}
function required<T>(value: T | null): T {
  if (!value) throw new Error("ConnectionProvider is missing");
  return value;
}
/**
 * Everything, which re-renders on any change to the connection or the
 * catalog. Prefer `useCommand`, `usePairing` or `useCatalog` when that is
 * all a component reads.
 */
export function useConnection(): Connection {
  const actions = required(use(ActionsContext));
  const state = required(use(StateContext));
  const store = required(use(CatalogContext));
  const catalog = useSyncExternalStore(store.subscribe, store.get);
  return useMemo(() => ({ ...state, ...actions, catalog }), [state, actions, catalog]);
}
/** The call to the desktop; the same function for the life of the app. */
export function useCommand() {
  return required(use(ActionsContext)).command;
}
/** `refresh`, `select`, `pair`, `disconnect` and `command`; none ever change. */
export function useConnectionActions() {
  return required(use(ActionsContext));
}
/** The paired desktop, if any; changes only on pairing and disconnecting. */
export function usePairing() {
  return required(use(StateContext)).pairing;
}
/**
 * A part of the catalog; re-renders only when what `select` returns changes,
 * so it must return the same value for the same data (a field, not a copy).
 */
export function useCatalog<T>(select: (catalog: Catalog) => T): T {
  const store = required(use(CatalogContext));
  return useSyncExternalStore(store.subscribe, () => select(store.get()));
}
export type Conversation = {
  id: string;
  messages: Message[];
  /** Missing from desktops that predate turn-shaped transcripts. */
  turns?: Turn[];
  busy: boolean;
  /** What the agent waits on the user to answer; absent from an older desktop. */
  question?: QuestionPrompt | null;
};

/** What the desktop lists this phone as, e.g. "Nick's iPhone". */
function deviceName() {
  return (
    Constants.deviceName?.trim() || (Platform.OS === "ios" ? "iPhone" : "Phone")
  );
}

/** Whether the desktop can show and change a Mono's profile, reset or delete
 *  it, and make new ones; where it can't, the phone leaves them out. */
export function useMonoTools() {
  return useCatalog((catalog) => catalog.monoTools === true);
}

/** Whether the desktop lets the phone edit a Mono's soul and memory. */
export function useMonoFiles() {
  return useCatalog((catalog) => catalog.monoFiles === true);
}

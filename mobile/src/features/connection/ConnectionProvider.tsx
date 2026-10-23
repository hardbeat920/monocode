import {
  createContext,
  use,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
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
  Turn,
} from "@/features/workspace/types";
import {
  nativeCall,
  openWatch,
  nextUpdates,
  closeWatch,
  parseInvitation,
  request,
  type Pairing,
  type Desktop,
  type Identity,
} from "./nativeLink";
import { normalizeSelection, type MobileSelection } from "./selection";

export type Catalog = {
  projects: Project[];
  monos: Mono[];
  sessions: SessionSummary[];
  addresses?: Desktop["addresses"];
  selection?: MobileSelection | null;
  revision?: number;
  selectionImported?: boolean;
};
const EMPTY: Catalog = { projects: [], monos: [], sessions: [] };
const PAIRING = "monocode.mobile.pairing";
const IDENTITY = "monocode.mobile.identity";
const key = (fingerprint: string, kind: string) =>
  `monocode.mobile.${fingerprint}.${kind}`;
type Connection = {
  restoring: boolean;
  pairing: Pairing | null;
  catalog: Catalog;
  selection: MobileSelection | null;
  connected: boolean;
  working: boolean;
  error: string | null;
  pair: (text: string) => Promise<void>;
  refresh: () => Promise<void>;
  select: (
    selection: MobileSelection,
    base?: MobileSelection | null,
  ) => Promise<void>;
  disconnect: () => Promise<void>;
  command: <T>(method: string, params?: object, id?: string) => Promise<T>;
};
const Context = createContext<Connection | null>(null);
export function ConnectionProvider({ children }: { children: ReactNode }) {
  const [restoring, setRestoring] = useState(true);
  const [pairing, setPairing] = useState<Pairing | null>(null);
  const [catalog, setCatalog] = useState<Catalog>(EMPTY);
  const [selection, setSelection] = useState<MobileSelection | null>(null);
  const [connected, setConnected] = useState(false);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const active = useRef<Pairing | null>(null);
  const refreshing = useRef<Pairing | null>(null);
  const chosenNow = useRef<MobileSelection | null>(null);
  const legacySelection = useRef<MobileSelection | null>(null);
  const revision = useRef(-1);
  const saving = useRef(Promise.resolve());
  const lastCatalog = useRef<string | null>(null);
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
        chosenNow.current = readSelection(chosen);
        legacySelection.current = chosenNow.current;
        setSelection(chosenNow.current);
        if (saved) {
          try {
            const cached = JSON.parse(saved) as Catalog;
            if (
              Array.isArray(cached.projects) &&
              Array.isArray(cached.monos) &&
              Array.isArray(cached.sessions)
            )
              setCatalog(cached);
          } catch {
            /* Refresh repairs a corrupt optional workspace cache. */
          }
        }
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
  }, []);
  // One ordered cache write queue and a server revision prevent an older list
  // response from overwriting a newer pushed workspace (including on disk).
  const accept = useCallback(async (value: Pairing, next: Catalog) => {
    if (active.current !== value) return;
    if (
      !Array.isArray(next.projects) ||
      !Array.isArray(next.monos) ||
      !Array.isArray(next.sessions)
    )
      throw new Error("Desktop sent an invalid workspace");
    if (next.revision !== undefined && next.revision < revision.current) return;
    if (next.revision !== undefined) revision.current = next.revision;
    const addressesChanged = !!next.addresses?.length &&
      JSON.stringify(next.addresses) !== JSON.stringify(value.desktop.addresses);
    if (next.addresses?.length) value.desktop.addresses = next.addresses;
    if (next.selection !== undefined) {
      const chosen = normalizeSelection(next.selection);
      if (JSON.stringify(chosen) !== JSON.stringify(chosenNow.current)) {
        chosenNow.current = chosen;
        setSelection(chosen);
      }
    }
    const { addresses: _, selectionImported: _imported, ...workspace } = next;
    const serialized = JSON.stringify(workspace);
    if (serialized !== lastCatalog.current || addressesChanged) {
      lastCatalog.current = serialized;
      setCatalog((previous) => withUnchanged(previous, next));
      const chosen = chosenNow.current;
      const write = saving.current
        .catch(() => {})
        .then(async () => {
          if (active.current !== value) return;
          await Storage.setItem(PAIRING, JSON.stringify(value.desktop));
          await Storage.setItem(
            key(value.desktop.fingerprint, "catalog"),
            serialized,
          );
          if (chosen !== null)
            await Storage.setItem(
              key(value.desktop.fingerprint, "selection"),
              JSON.stringify(chosen),
            );
        });
      saving.current = write;
      // A cache failure must not suppress an already received live update.
      await write.catch(() => {});
    }
    if (active.current === value) {
      setConnected(true);
      setError(null);
    }
  }, []);
  const refresh = useCallback(async () => {
    const value = active.current;
    if (!value || refreshing.current === value) return;
    const version = generation.current;
    refreshing.current = value;
    try {
      let next = await request<Catalog>(value, "workspace.list");
      if (active.current !== value || version !== generation.current) return;
      // Import each installation's legacy choice once. The desktop records
      // the paired identity so stale caches cannot re-import after shared edits.
      if (
        next.selectionImported === false &&
        legacySelection.current !== null
      ) {
        next = await request<Catalog>(value, "workspace.select", {
          selection: legacySelection.current,
          base: null,
          initialize: true,
        });
      }
      if (next.selectionImported === true) legacySelection.current = null;
      await accept(value, next);
    } catch (reason) {
      if (active.current === value && version === generation.current) {
        setConnected(false);
        setError(reason instanceof Error ? reason.message : String(reason));
      }
    } finally {
      if (refreshing.current === value) refreshing.current = null;
    }
  }, [accept]);
  useEffect(() => {
    if (!pairing) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      if (AppState.currentState === "active") await refresh();
      if (!disposed) timer = setTimeout(() => void poll(), 5000);
    };
    void poll();
    const subscription = AppState.addEventListener("change", (state) => {
      if (state === "active") void refresh();
    });
    return () => {
      disposed = true;
      clearTimeout(timer);
      subscription.remove();
    };
  }, [pairing, refresh]);
  useEffect(() => {
    if (!pairing) return;
    let disposed = false;
    let watchId: number | undefined;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let running = false;
    const run = async () => {
      if (disposed || running || AppState.currentState !== "active") return;
      running = true;
      try {
        const id = await openWatch(pairing, "workspace.watch", {});
        if (disposed || AppState.currentState !== "active") {
          await closeWatch(id);
          return;
        }
        watchId = id;
        while (!disposed && watchId === id) {
          const frames = await nextUpdates<Catalog>(id, 1000);
          if (disposed || watchId !== id) break;
          for (const frame of frames) {
            // Initialization is handled by refresh, with the old local choice.
            if (frame.selection === null && chosenNow.current !== null)
              continue;
            await accept(pairing, frame);
          }
        }
      } catch {
        // Polling remains a recovery path, including for older desktops.
        if (!disposed) void refresh();
      } finally {
        if (watchId !== undefined) void closeWatch(watchId);
        watchId = undefined;
        running = false;
        if (!disposed) retry = setTimeout(() => void run(), 1000);
      }
    };
    void run();
    const subscription = AppState.addEventListener("change", (state) => {
      clearTimeout(retry);
      if (state === "active") {
        void run();
      } else if (watchId !== undefined) {
        const id = watchId;
        watchId = undefined;
        void closeWatch(id);
      }
    });
    return () => {
      disposed = true;
      clearTimeout(retry);
      subscription.remove();
      if (watchId !== undefined) void closeWatch(watchId);
    };
  }, [pairing, accept, refresh]);
  const pair = useCallback(
    async (text: string) => {
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
        setCatalog(EMPTY);
        lastCatalog.current = null;
        revision.current = -1;
        chosenNow.current = readSelection(chosen);
        legacySelection.current = chosenNow.current;
        setSelection(chosenNow.current);
        await refresh();
      } catch (reason) {
        setError(reason instanceof Error ? reason.message : String(reason));
      } finally {
        pairingNow.current = false;
        setWorking(false);
      }
    },
    [refresh],
  );
  const select = useCallback(
    async (
      selection: MobileSelection,
      base: MobileSelection | null = chosenNow.current,
    ) => {
      const value = active.current;
      if (!value) return;
      const next = await request<Catalog>(value, "workspace.select", {
        selection,
        base,
      });
      await accept(value, next);
    },
    [accept],
  );
  const disconnect = useCallback(async () => {
    const value = active.current;
    generation.current++;
    active.current = null;
    setPairing(null);
    setConnected(false);
    setCatalog(EMPTY);
    lastCatalog.current = null;
    chosenNow.current = null;
    legacySelection.current = null;
    revision.current = -1;
    setSelection(null);
    setError(null);
    await Storage.removeItem(PAIRING);
    if (value) {
      void request(value, "device.leave").catch(() => {});
      await Storage.removeItem(key(value.desktop.fingerprint, "catalog"));
    }
    // Keep this installation's identity, so a new invitation can recover cleanly.
  }, []);
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
  // Every screen reads this; a new object each render would re-render them all.
  const value = useMemo(
    () => ({
      restoring,
      pairing,
      catalog,
      selection,
      connected,
      working,
      error,
      pair,
      refresh,
      select,
      disconnect,
      command,
    }),
    [
      restoring,
      pairing,
      catalog,
      selection,
      connected,
      working,
      error,
      pair,
      refresh,
      select,
      disconnect,
      command,
    ],
  );
  return <Context value={value}>{children}</Context>;
}
/**
 * The new catalog, keeping the projects, monos and sessions that did not
 * change as they were, so rows memoized on them skip re-rendering.
 */
function withUnchanged(previous: Catalog, next: Catalog): Catalog {
  return {
    ...next,
    projects: sameItems(previous.projects, next.projects),
    monos: sameItems(previous.monos, next.monos),
    sessions: sameItems(previous.sessions, next.sessions),
  };
}
function sameItems<T extends { id: string }>(previous: T[], next: T[]): T[] {
  const known = new Map(previous.map((item) => [item.id, item]));
  const merged = next.map((item) => {
    const before = known.get(item.id);
    return before && JSON.stringify(before) === JSON.stringify(item)
      ? before
      : item;
  });
  return merged.length === previous.length &&
    merged.every((item, i) => item === previous[i])
    ? previous
    : merged;
}
function readSelection(saved: string | null): MobileSelection | null {
  try {
    return saved ? normalizeSelection(JSON.parse(saved)) : null;
  } catch {
    return null;
  }
}
export function useConnection() {
  const value = use(Context);
  if (!value) throw new Error("ConnectionProvider is missing");
  return value;
}
export type Conversation = {
  id: string;
  messages: Message[];
  /** Missing from desktops that predate turn-shaped transcripts. */
  turns?: Turn[];
  busy: boolean;
};

/** What the desktop lists this phone as, e.g. "Nick's iPhone". */
function deviceName() {
  return (
    Constants.deviceName?.trim() || (Platform.OS === "ios" ? "iPhone" : "Phone")
  );
}

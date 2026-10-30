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
import { normalizeSelection, type MobileSelection } from "./selection";

export type Catalog = {
  projects: Project[];
  monos: Mono[];
  sessions: SessionSummary[];
  addresses?: Desktop["addresses"];
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
  select: (selection: MobileSelection) => Promise<void>;
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
  const refreshing = useRef(false);
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
        setSelection(chosen ? normalizeSelection(JSON.parse(chosen)) : null);
        if (saved) setCatalog(JSON.parse(saved));
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
      if (next.addresses?.length) {
        value.desktop.addresses = next.addresses;
        await Storage.setItem(PAIRING, JSON.stringify(value.desktop));
      }
      // The poll repeats every few seconds; only a changed workspace is saved
      // and re-rendered.
      const { addresses: _, ...workspace } = next;
      const serialized = JSON.stringify(workspace);
      if (serialized !== lastCatalog.current) {
        await Storage.setItem(key(value.desktop.fingerprint, "catalog"), serialized);
        if (active.current !== value) return;
        lastCatalog.current = serialized;
        setCatalog((previous) => withUnchanged(previous, next));
      }
      setConnected(true);
      setError(null);
    } catch (reason) {
      if (active.current === value) {
        setConnected(false);
        setError(reason instanceof Error ? reason.message : String(reason));
      }
    } finally {
      refreshing.current = false;
    }
  }, []);
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
      setCatalog(EMPTY);
      lastCatalog.current = null;
      setSelection(chosen ? normalizeSelection(JSON.parse(chosen)) : null);
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      pairingNow.current = false;
      setWorking(false);
    }
  }, [refresh]);
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
    setCatalog(EMPTY);
    lastCatalog.current = null;
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
    return before && JSON.stringify(before) === JSON.stringify(item) ? before : item;
  });
  return merged.length === previous.length && merged.every((item, i) => item === previous[i])
    ? previous
    : merged;
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
  /** What the agent waits on the user to answer; absent from an older desktop. */
  question?: QuestionPrompt | null;
};

/** What the desktop lists this phone as, e.g. "Nick's iPhone". */
function deviceName() {
  return (
    Constants.deviceName?.trim() || (Platform.OS === "ios" ? "iPhone" : "Phone")
  );
}

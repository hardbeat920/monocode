import {
  createContext,
  useCallback,
  useContext,
  useRef,
  useState,
  useSyncExternalStore,
  type Dispatch,
  type SetStateAction,
} from "react";
import type { Session } from "./session";

type Listener = () => void;

/** One authoritative snapshot; text delivery only wakes its own consumers. */
export class SessionStateStore {
  private sessions: Session[];
  private byId: Map<string, Session>;
  readonly ref: { current: Session[] };
  private shellListeners = new Set<Listener>();
  private observers = new Set<Listener>();
  private listeners = new Map<string, Set<Listener>>();

  constructor(sessions: Session[]) {
    this.sessions = sessions;
    this.byId = new Map(sessions.map((session) => [session.id, session]));
    this.ref = { current: sessions };
  }

  snapshot = () => this.sessions;
  session = (id: string) => this.byId.get(id);
  subscribeShell = (listener: Listener) => {
    this.shellListeners.add(listener);
    return () => {
      this.shellListeners.delete(listener);
    };
  };
  subscribe = (id: string, listener: Listener) => {
    let listeners = this.listeners.get(id);
    if (!listeners) this.listeners.set(id, (listeners = new Set()));
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
      if (!listeners.size) this.listeners.delete(id);
    };
  };
  observe = (listener: Listener) => {
    this.observers.add(listener);
    return () => {
      this.observers.delete(listener);
    };
  };

  setState: Dispatch<SetStateAction<Session[]>> = (update) => {
    this.update(update);
  };

  update(update: SetStateAction<Session[]>, textOnly = false): Session[] {
    const previous = this.sessions;
    const next = typeof update === "function" ? update(previous) : update;
    if (next === previous) return next;
    const before = this.byId;
    this.sessions = next;
    this.ref.current = next;
    this.byId = new Map(next.map((session) => [session.id, session]));
    // Notify after publishing the entire batch. Synchronous consumers (send,
    // stop, persistence, provider repair) always read the newest output.
    for (const [id, listeners] of this.listeners) {
      if (before.get(id) !== this.byId.get(id)) {
        for (const listener of listeners) listener();
      }
    }
    for (const observer of this.observers) observer();
    if (!textOnly) for (const listener of this.shellListeners) listener();
    return next;
  }
}

export const SessionStateContext = createContext<SessionStateStore | null>(
  null,
);

export function useWorkspaceSessionState(initial: () => Session[]) {
  const [store] = useState(() => new SessionStateStore(initial()));
  const sessions = useSyncExternalStore(store.subscribeShell, store.snapshot);
  return {
    sessions,
    setSessions: store.setState,
    sessionsRef: store.ref,
    store,
  };
}

const noSubscription = () => () => {};

/** Hidden panes keep their last render and read the latest snapshot on reveal. */
export function useSessionSnapshot<T extends Session | undefined>(
  fallback: T,
  visible: boolean,
): T {
  const store = useContext(SessionStateContext);
  const id = fallback?.id;
  const retained = useRef({ store, id, fallback, value: fallback });
  // Shell metadata updates still apply to hidden panes. A visibility change
  // alone must retain the snapshot that was already on screen.
  if (
    retained.current.store !== store ||
    retained.current.id !== id ||
    retained.current.fallback !== fallback
  )
    retained.current = {
      store,
      id,
      fallback,
      value: (store && id ? (store.session(id) ?? fallback) : fallback) as T,
    };
  const subscribe = useCallback(
    (listener: Listener) => {
      return store && id && visible
        ? store.subscribe(id, listener)
        : noSubscription();
    },
    [store, id, visible],
  );
  const snapshot = useCallback(
    () =>
      (store && id
        ? visible
          ? (store.session(id) ?? fallback)
          : retained.current.value
        : fallback) as T,
    [store, id, fallback, visible],
  );
  const value = useSyncExternalStore(subscribe, snapshot);
  retained.current.value = value;
  return value;
}

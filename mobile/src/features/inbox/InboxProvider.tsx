import Storage from "expo-sqlite/kv-store";
import {
  createContext,
  use,
  useCallback,
  useEffect,
  useMemo,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import { AppState } from "react-native";

import { useConnection } from "@/features/connection/ConnectionProvider";
import { makeCommandId } from "@/features/connection/nativeLink";

import { createFixtureClient, FIXTURES_ENABLED } from "./fixtures";
import { actionMessage } from "./itemActions";
import { countItems, markMessage } from "./model";
import { useToast, type ToastState } from "@/ui/useToast";

import { useNow } from "./useNow";
import {
  createInboxClient,
  type InboxAction,
  type InboxActionResult,
  type InboxClient,
  type InboxItem,
  type InboxMarkAction,
} from "./protocol";
import { createReviewBook, type ReviewBook } from "./review";
import { createInboxStore, type Feed, type FeedQuery, type InboxStore } from "./store";

export type InboxToastState = ToastState;

type InboxContext = {
  store: InboxStore;
  client: InboxClient;
  /** The pending line comments, one list per pull request, on every screen. */
  review: ReviewBook;
  toast: InboxToastState | null;
  dismissToast: () => void;
  /** Marks an item, shown at once; Undo in the toast takes it back. */
  mark: (item: InboxItem, action: InboxMarkAction, until?: number) => Promise<void>;
  /** Runs a pull request action. Throws what the provider said, for the screen to show. */
  act: (item: InboxItem, action: InboxAction) => Promise<InboxActionResult>;
  /** Marks read everything in a list. */
  readAll: (query: FeedQuery) => Promise<void>;
  /** One line in the toast. */
  notify: (message: string) => void;
};

const Context = createContext<InboxContext | null>(null);

/**
 * The Inbox for the linked desktop: its client and store, and the one toast
 * that says what a mark did. A desktop whose catalog lacks `inbox` is left
 * alone: nothing is requested. `client` stands in for the desktop in tests and
 * on the fixture build.
 */
export function InboxProvider({
  children,
  client: injected,
}: {
  children: ReactNode;
  client?: InboxClient;
}) {
  const connection = useConnection();
  const { command, pairing, connected, catalog } = connection;
  const namespace = pairing?.desktop.fingerprint ?? "unpaired";
  const fixtures = !injected && FIXTURES_ENABLED;
  // Only the fixture client maps its items onto the catalog's projects; the
  // real one must not be rebuilt (losing pending marks) when a project is added.
  const projectKey = fixtures ? catalog.projects.map((project) => project.id).join("\n") : "";
  const client = useMemo<InboxClient>(
    () =>
      injected ??
      (fixtures
        ? createFixtureClient({ projectIds: projectKey ? projectKey.split("\n") : [] })
        : createInboxClient(command, makeCommandId)),
    [injected, fixtures, command, projectKey],
  );
  const store = useMemo(
    () =>
      createInboxStore({
        client,
        namespace: fixtures ? `${namespace}.fixtures` : namespace,
        storage: {
          get: (key) => Storage.getItemSync(key),
          set: (key, value) => Storage.setItemSync(key, value),
        },
        isActive: () => AppState.currentState === "active",
      }),
    [client, namespace, fixtures],
  );
  useEffect(() => () => store.dispose(), [store]);
  useEffect(() => {
    store.follow(catalog.inboxRev);
  }, [store, catalog.inboxRev]);
  const review = useMemo(() => createReviewBook(client), [client]);
  const linked = connected || fixtures || !!injected;
  useEffect(() => {
    store.setLinked(linked);
  }, [store, linked]);
  useEffect(() => {
    const subscription = AppState.addEventListener("change", (state) => {
      if (state === "active") store.wake();
    });
    return () => subscription.remove();
  }, [store]);

  const { toast, show, dismiss: dismissToast } = useToast();

  const mark = useCallback(
    async (item: InboxItem, action: InboxMarkAction, until?: number) => {
      try {
        const result = await store.mark(item.key, action, until);
        // Opening an item marks it read; that needs no toast.
        if (action === "seen") return;
        show(markMessage(action), () => {
          dismissToast();
          void result.undo().catch(() => show("Couldn't undo. The desktop didn't answer."));
        });
      } catch {
        show("Couldn't reach the desktop. Nothing changed.");
      }
    },
    [store, show, dismissToast],
  );

  const act = useCallback(
    async (item: InboxItem, action: InboxAction) => {
      const result = await store.act(item.key, action);
      show(actionMessage(action, result.queued));
      return result;
    },
    [store, show],
  );

  const readAll = useCallback(
    async (query: FeedQuery) => {
      try {
        await store.readAll(query);
        show("Marked all as read");
      } catch {
        show("Couldn't reach the desktop. Nothing changed.");
      }
    },
    [store, show],
  );

  const notify = useCallback((message: string) => show(message), [show]);

  const value = useMemo(
    () => ({ store, client, review, toast, dismissToast, mark, act, readAll, notify }),
    [store, client, review, toast, dismissToast, mark, act, readAll, notify],
  );
  return <Context value={value}>{children}</Context>;
}

/** Null outside the provider, so screens that may render without it just skip the Inbox. */
function useOptionalInbox() {
  return use(Context);
}

export function useInbox() {
  const value = use(Context);
  if (!value) throw new Error("InboxProvider is missing");
  return value;
}

/**
 * Whether the Inbox is shown: the desktop says it serves it, or this is the
 * fixture build. An older desktop's phone never sees its row in the drawer.
 */
export function useInboxSupported() {
  const inbox = useOptionalInbox();
  const catalog = useConnection().catalog;
  return !!inbox && (catalog.inbox === true || FIXTURES_ENABLED);
}

/** Whether the desktop answers `inbox.readAll`, `inbox.action` and `inbox.comment`. */
export function useInboxActionsSupported() {
  return useConnection().catalog.inboxActions === true || FIXTURES_ENABLED;
}

const IDLE_FEED = (key: string): Feed => ({
  key,
  items: [],
  loaded: false,
  loadingMore: false,
  fromCache: false,
  offline: false,
  error: null,
  stale: false,
  fetchedAt: null,
  providers: [],
  counts: { unread: 0, needsMe: 0, done: 0 },
});

/** A list, watched (read now and polled) while `watching`. */
export function useInboxFeed(query: FeedQuery, watching = true) {
  const inbox = useOptionalInbox();
  const { scope, projectId, source, q, facets } = query;
  const stable = useMemo<FeedQuery>(
    () => ({ scope, projectId, source, q, facets }),
    [scope, projectId, source, q, facets],
  );
  const store = inbox?.store;
  const subscribe = useCallback(
    (listener: () => void) => store?.subscribe(listener) ?? (() => {}),
    [store],
  );
  const idle = useMemo(() => IDLE_FEED("idle"), []);
  const feed = useSyncExternalStore(
    subscribe,
    () => store?.getFeed(stable) ?? idle,
    () => idle,
  );
  useEffect(() => {
    if (!store || !watching) return;
    return store.watch(stable);
  }, [store, stable, watching]);
  return {
    feed,
    /** Pull to refresh: the desktop fetches from its providers before it answers. */
    refresh: useCallback(
      () => store?.refresh(stable, { force: true }) ?? Promise.resolve(),
      [store, stable],
    ),
    loadMore: useCallback(() => store?.loadMore(stable) ?? Promise.resolve(), [store, stable]),
  };
}

/** The unread count across every project, for the drawer: 0 without the Inbox. */
export function useInboxBadge() {
  const supported = useInboxSupported();
  const { feed } = useInboxFeed({ scope: "all" }, supported);
  const now = useNow(supported);
  return supported ? countItems(feed.items, undefined, now).unread : 0;
}

import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import InboxListScreen from "@/app/inbox-list";
import { InboxProvider } from "@/features/inbox/InboxProvider";
import { createFixtureClient } from "@/features/inbox/fixtures";
import type { InboxList, InboxListParams } from "@/features/inbox/protocol";

// What redraws the Inbox. Everything around the screen is a stand-in; the store,
// the screen, the page and the rows are the real ones, so a row drawn again here
// is one the phone would draw again.

const seen = vi.hoisted(() => ({
  rows: 0,
  pages: 0,
  catalog: {
    projects: [{ id: "p1", name: "monocode" }],
    sessions: [] as { id: string }[],
    inbox: true,
    inboxActions: true,
  },
  listeners: new Set<() => void>(),
}));

vi.mock("expo-sqlite/kv-store", () => ({
  default: { getItemSync: () => null, setItemSync: () => {} },
}));
vi.mock("expo-haptics", () => ({
  selectionAsync: async () => {},
  notificationAsync: async () => {},
  NotificationFeedbackType: { Success: "success" },
}));
vi.mock("react-native", () => ({
  ActivityIndicator: () => null,
  AppState: { currentState: "active", addEventListener: () => ({ remove: () => {} }) },
  Pressable: ({ children }: { children?: ReactNode }) => <button>{children}</button>,
  RefreshControl: () => null,
  ScrollView: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  StyleSheet: { create: (styles: unknown) => styles },
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  useColorScheme: () => "dark",
  useWindowDimensions: () => ({ width: 390, height: 844, fontScale: 1 }),
}));
vi.mock("react-native-gesture-handler/ReanimatedSwipeable", () => ({
  default: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
}));
vi.mock("react-native-worklets", () => ({ scheduleOnRN: () => {} }));
vi.mock("react-native-reanimated", () => {
  // Each call is a new object, as the real builders' are.
  const fresh = () => ({ duration: fresh, reduceMotion: fresh });
  type Entry = { id: string };
  return {
    default: {
      View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
      FlatList: ({
        data,
        renderItem,
        keyExtractor,
        ListHeaderComponent,
        ListEmptyComponent,
      }: {
        data: Entry[];
        renderItem: (info: { item: Entry }) => ReactNode;
        keyExtractor: (entry: Entry) => string;
        ListHeaderComponent?: ReactNode;
        ListEmptyComponent?: ReactNode;
      }) => (
        <div>
          {ListHeaderComponent}
          {data.length === 0 ? ListEmptyComponent : null}
          {data.map((entry) => (
            <div key={keyExtractor(entry)}>{renderItem({ item: entry })}</div>
          ))}
        </div>
      ),
    },
    Easing: { inOut: () => 0, quad: 0 },
    FadeOut: fresh(),
    LinearTransition: fresh(),
    ReduceMotion: { System: "system" },
    useAnimatedReaction: () => {},
    useAnimatedStyle: () => ({}),
    useReducedMotion: () => false,
    useSharedValue: (initial: unknown) => ({ get: () => initial, set: () => {} }),
    withRepeat: () => 0,
    withSequence: () => 0,
    withTiming: () => 0,
  };
});
vi.mock("@/features/connection/ConnectionProvider", async () => {
  const { useSyncExternalStore } = await import("react");
  const subscribe = (listener: () => void) => {
    seen.listeners.add(listener);
    return () => void seen.listeners.delete(listener);
  };
  return {
    useConnection: () => ({
      catalog: useSyncExternalStore(subscribe, () => seen.catalog),
      command: async () => ({}),
      connected: true,
      pairing: null,
    }),
  };
});
vi.mock("@/features/connection/nativeLink", () => ({ makeCommandId: () => "id" }));
vi.mock("@/features/files/FileSearchBar", () => ({ FileSearchBar: () => null, SEARCH_BAR: 44 }));
vi.mock("@/features/mascots/ProjectMascot", () => ({ ProjectMascot: () => null }));
vi.mock("@/features/sessions/SessionScreen", () => ({
  HEADER_HEIGHT: 44,
  SheetLayout: ({ children }: { children: ReactNode }) => children,
  // Read once each time the page is drawn.
  useSessionLayout: () => {
    seen.pages++;
    return { pageTop: 100, dockBottom: 20 };
  },
}));
vi.mock("expo-router", () => ({
  router: { back: () => {}, canGoBack: () => false, push: () => {} },
  Stack: { Screen: () => null },
  useLocalSearchParams: () => ({}),
}));
vi.mock("react-native-safe-area-context", () => ({
  useSafeAreaInsets: () => ({ top: 0, bottom: 0, left: 0, right: 0 }),
}));
vi.mock("@/ui/useNativeHeaderHeight", () => ({ useNativeHeaderHeight: () => 44 }));
vi.mock("@/features/workspace/projectColors", () => ({ projectColor: () => "#000" }));
vi.mock("@/ui/GlassSegments", () => ({ GlassSegments: () => null }));
vi.mock("@/ui/GradualBlur", () => ({ GradualBlur: () => null }));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/ui/ChoiceMenu", () => ({ ChoiceMenu: () => null }));
vi.mock("@/ui/SwipeAction", () => ({
  ACTION_GAP: 8,
  ACTION_SIZE: 40,
  ActionFace: () => null,
  SwipeAction: () => null,
  SwipeActions: () => null,
}));
vi.mock("@/features/inbox/InboxFilters", () => ({
  Chip: () => null,
  FilterChips: () => null,
  FilterSheet: () => null,
  placeName: () => "",
}));
vi.mock("@/features/inbox/ProviderMark", () => ({ ProviderMark: () => null, SOURCE_NAMES: {} }));
vi.mock("@/features/inbox/GroupMenu", () => ({ GroupMenu: () => null }));
vi.mock("@/features/inbox/InboxToast", () => ({ InboxToast: () => null }));
// Drawn once per row each time the row itself is drawn.
vi.mock("@/features/inbox/ItemMark", () => ({
  ItemMark: () => {
    seen.rows++;
    return null;
  },
}));

const NOW = new Date("2026-11-01T14:00:00").getTime();

/** The fixture desktop, its replies new objects each time as over the link. */
function linkedClient() {
  const client = createFixtureClient({
    projectIds: ["p1"],
    now: () => NOW,
    controls: { latencyMs: 0 },
  });
  const list = client.list;
  return {
    ...client,
    list: (params: InboxListParams) =>
      list(params).then((page) => ({
        ...(JSON.parse(JSON.stringify(page)) as InboxList),
        fetchedAt: NOW,
      })),
  };
}

describe("what redraws the Inbox's rows", () => {
  let root: Root;
  

  async function open() {
    const host = document.createElement("div");
    root = createRoot(host);
    await act(async () => {
      root.render(
        <InboxProvider client={linkedClient()}>
          <InboxListScreen />
        </InboxProvider>,
      );
      await vi.advanceTimersByTimeAsync(10);
    });
    expect(seen.rows).toBeGreaterThan(0);
    seen.rows = 0;
    seen.pages = 0;
  }

  beforeEach(() => {
    vi.useFakeTimers({ now: NOW });
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    seen.rows = 0;
  });
  afterEach(() => {
    act(() => root.unmount());
    vi.useRealTimers();
  });

  it("isn't a poll that finds nothing new", async () => {
    await open();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(20_000);
    });
    expect(seen.rows).toBe(0);
  });

  it("isn't an agent's session moving on the desktop", async () => {
    await open();
    await act(async () => {
      seen.catalog = { ...seen.catalog, sessions: [{ id: "s1" }] };
      for (const listener of seen.listeners) listener();
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(seen.pages).toBe(0);
    expect(seen.rows).toBe(0);
  });
});

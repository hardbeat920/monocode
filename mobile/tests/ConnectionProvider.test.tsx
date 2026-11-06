import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  ConnectionProvider,
  useConnection,
  type Catalog,
} from "@/features/connection/ConnectionProvider";
import { selectedWorkspace } from "@/features/connection/selection";

const link = vi.hoisted(() => ({
  storage: new Map<string, string>(),
  request: vi.fn(),
  openWatch: vi.fn(),
  nextUpdates: vi.fn(),
  closeWatch: vi.fn(),
  listeners: new Set<(state: string) => void>(),
  state: "active",
}));
vi.mock("react-native", () => ({
  Platform: { OS: "ios" },
  AppState: {
    get currentState() {
      return link.state;
    },
    addEventListener: (_: string, fn: (state: string) => void) => {
      link.listeners.add(fn);
      return { remove: () => link.listeners.delete(fn) };
    },
  },
}));
vi.mock("expo-constants", () => ({ default: {} }));
vi.mock("expo-secure-store", () => ({ getItemAsync: async () => "{}" }));
vi.mock("expo-sqlite/kv-store", () => ({
  default: {
    getItem: async (key: string) => link.storage.get(key) ?? null,
    setItem: async (key: string, value: string) =>
      void link.storage.set(key, value),
    removeItem: async (key: string) => void link.storage.delete(key),
  },
}));
vi.mock("@/features/connection/nativeLink", () => ({
  request: link.request,
  openWatch: link.openWatch,
  nextUpdates: link.nextUpdates,
  closeWatch: link.closeWatch,
  nativeCall: vi.fn(),
  parseInvitation: vi.fn(),
}));
(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
const selection = (projectIds: string[]) => ({ projectIds, monoIds: [] });
const catalog = (ids: string[], revision: number): Catalog => ({
  projects: ["a", "b", "c"].map((id) => ({
    id,
    name: id,
    path: id,
  })) as Catalog["projects"],
  monos: [],
  sessions: [],
  selection: selection(ids),
  revision,
});
describe("connected mobile workspaces", () => {
  let roots: ReturnType<typeof createRoot>[];
  let clients: ReturnType<typeof useConnection>[];
  let waiting: Map<number, ReturnType<typeof deferred<Catalog[]>>>;
  let server: Catalog;
  const flush = () =>
    act(async () => {
      for (let i = 0; i < 40; i++) await Promise.resolve();
    });
  function Probe({ index }: { index: number }) {
    clients[index] = useConnection();
    return null;
  }
  const mount = async (index = 0) => {
    const root = createRoot(document.createElement("div"));
    roots.push(root);
    act(() =>
      root.render(
        <ConnectionProvider>
          <Probe index={index} />
        </ConnectionProvider>,
      ),
    );
    await flush();
  };
  const ids = (index: number) =>
    selectedWorkspace(
      clients[index].catalog,
      clients[index].selection,
    ).projects.map((p) => p.id);
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    link.storage.clear();
    link.listeners.clear();
    link.state = "active";
    roots = [];
    clients = [];
    waiting = new Map();
    server = catalog(["a"], 1);
    link.storage.set(
      "monocode.mobile.pairing",
      JSON.stringify({ fingerprint: "desktop" }),
    );
    link.storage.set(
      "monocode.mobile.desktop.selection",
      JSON.stringify(selection(["a"])),
    );
    let nextId = 0;
    link.openWatch.mockImplementation(async () => ++nextId);
    link.nextUpdates.mockImplementation((id: number) => {
      const next = deferred<Catalog[]>();
      waiting.set(id, next);
      return next.promise;
    });
    link.closeWatch.mockResolvedValue(undefined);
    link.request.mockImplementation(
      async (
        _: unknown,
        method: string,
        params: { selection: Catalog["selection"] },
      ) => {
        if (method === "workspace.select") {
          server = {
            ...server,
            selection: params.selection,
            revision: server.revision! + 1,
          };
          for (const next of waiting.values()) next.resolve([server]);
        }
        return server;
      },
    );
  });
  afterEach(() => {
    act(() => roots.forEach((root) => root.unmount()));
    vi.useRealTimers();
  });

  it("shows a project selected on iPhone immediately on Android without a poll", async () => {
    await mount(0);
    await mount(1);
    expect(ids(1)).toEqual(["a"]);
    await act(async () => clients[0].select(selection(["a", "b"])));
    await flush();
    expect(ids(0)).toEqual(["a", "b"]);
    expect(ids(1)).toEqual(["a", "b"]);
    expect(
      link.request.mock.calls.filter(
        ([, method]) => method === "workspace.list",
      ),
    ).toHaveLength(2);
    expect(
      JSON.parse(link.storage.get("monocode.mobile.desktop.selection")!),
    ).toEqual(selection(["a", "b"]));
  });

  it("does not let an older refresh overwrite a newer watch frame or persisted selection", async () => {
    await mount();
    const old = deferred<Catalog>();
    link.request.mockReturnValueOnce(old.promise);
    let refreshing!: Promise<void>;
    act(() => {
      refreshing = clients[0].refresh();
    });
    waiting.get(1)!.resolve([catalog(["a", "b"], 3)]);
    await flush();
    await act(async () => {
      old.resolve(catalog(["a"], 2));
      await refreshing;
    });
    expect(ids(0)).toEqual(["a", "b"]);
    expect(
      JSON.parse(link.storage.get("monocode.mobile.desktop.selection")!),
    ).toEqual(selection(["a", "b"]));
  });

  it("repairs stale and corrupt caches and resumes a failed watch with a fresh snapshot", async () => {
    link.storage.set("monocode.mobile.desktop.catalog", "broken json");
    link.storage.set(
      "monocode.mobile.desktop.selection",
      JSON.stringify(selection(["c"])),
    );
    await mount();
    expect(ids(0)).toEqual(["a"]);
    server = catalog(["a", "b"], 4);
    waiting.get(1)!.reject(new Error("connection lost"));
    await flush();
    expect(ids(0)).toEqual(["a", "b"]);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(link.openWatch).toHaveBeenCalledTimes(2);
    waiting.get(2)!.resolve([catalog(["a", "b", "c"], 5)]);
    await flush();
    expect(ids(0)).toEqual(["a", "b", "c"]);
  });

  it("persists changed desktop addresses even when the workspace is unchanged", async () => {
    await mount();
    server = { ...server, addresses: [{ host: "desktop.new", port: 38416 }] };
    await act(async () => clients[0].refresh());
    expect(JSON.parse(link.storage.get("monocode.mobile.pairing")!).addresses)
      .toEqual([{ host: "desktop.new", port: 38416 }]);
  });

  it("imports a legacy local choice once before adopting the shared selection", async () => {
    server = { ...catalog(["a"], 1), selectionImported: false };
    link.storage.set(
      "monocode.mobile.desktop.selection",
      JSON.stringify(selection(["b"])),
    );
    link.request.mockImplementation(
      async (
        _: unknown,
        method: string,
        params: { selection: Catalog["selection"] },
      ) => {
        if (method === "workspace.select") {
          expect(params).toEqual({
            selection: selection(["b"]),
            base: null,
            initialize: true,
          });
          server = { ...catalog(["a", "b"], 2), selectionImported: true };
        }
        return server;
      },
    );
    await mount();
    expect(ids(0)).toEqual(["a", "b"]);
    await act(async () => clients[0].refresh());
    expect(
      link.request.mock.calls.filter(
        ([, method]) => method === "workspace.select",
      ),
    ).toHaveLength(1);
  });

  it("closes the watch in background and refreshes on foreground", async () => {
    await mount();
    act(() => {
      link.state = "background";
      link.listeners.forEach((fn) => fn(link.state));
    });
    expect(link.closeWatch).toHaveBeenCalledWith(1);
    server = catalog(["b"], 6);
    act(() => {
      link.state = "active";
      link.listeners.forEach((fn) => fn(link.state));
    });
    await flush();
    expect(ids(0)).toEqual(["b"]);
  });
});

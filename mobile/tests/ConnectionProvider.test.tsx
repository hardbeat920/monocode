import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const world = vi.hoisted(() => ({
  appState: "active",
  appListeners: new Set<(state: string) => void>(),
  store: new Map<string, string>(),
  writes: [] as string[],
  requests: 0,
  catalog: undefined as unknown as Record<string, unknown>,
  addresses: [{ host: "10.0.0.2", port: 1 }],
  fail: false,
}));
vi.mock("react-native", () => ({
  Platform: { OS: "ios" },
  AppState: {
    get currentState() {
      return world.appState;
    },
    addEventListener: (_: string, listener: (state: string) => void) => {
      world.appListeners.add(listener);
      return { remove: () => world.appListeners.delete(listener) };
    },
  },
}));
vi.mock("expo-constants", () => ({ default: { deviceName: "Phone" } }));
vi.mock("expo-secure-store", () => ({
  WHEN_UNLOCKED_THIS_DEVICE_ONLY: 1,
  getItemAsync: async () => JSON.stringify({ certificate: "c", privateKey: "k" }),
  setItemAsync: async () => {},
}));
vi.mock("expo-sqlite/kv-store", () => ({
  default: {
    getItem: async (key: string) => world.store.get(key) ?? null,
    setItem: async (key: string, value: string) => {
      world.writes.push(key);
      world.store.set(key, value);
    },
    removeItem: async (key: string) => void world.store.delete(key),
  },
}));
vi.mock("@/features/connection/nativeLink", () => ({
  nativeCall: async () => ({}),
  parseInvitation: () => ({}),
  request: async () => {
    world.requests++;
    if (world.fail) throw new Error("offline");
    return structuredClone({ ...world.catalog, addresses: world.addresses });
  },
}));

import {
  ConnectionProvider,
  useCatalog,
  useCommand,
  useConnection,
} from "@/features/connection/ConnectionProvider";

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

const renders = { command: 0, monoTools: 0, everything: 0 };
const bump = (name: keyof typeof renders) => {
  renders[name]++;
};
function CommandOnly() {
  useCommand();
  bump("command");
  return null;
}
function ToolsOnly() {
  useCatalog((catalog) => catalog.monoTools === true);
  bump("monoTools");
  return null;
}
function Everything() {
  useConnection();
  bump("everything");
  return null;
}

const session = (id: string, busy = false) => ({ id, busy, title: id });
let root: ReturnType<typeof createRoot>;
const settle = () => act(async () => void (await vi.advanceTimersByTimeAsync(0)));
const advance = (ms: number) => act(async () => void (await vi.advanceTimersByTimeAsync(ms)));
const catalogWrites = () => world.writes.filter((key) => key.endsWith(".catalog")).length;
const pairingWrites = () => world.writes.filter((key) => key === "monocode.mobile.pairing").length;

beforeEach(async () => {
  vi.useFakeTimers();
  world.appState = "active";
  world.appListeners.clear();
  world.store.clear();
  world.writes.length = 0;
  world.requests = 0;
  world.fail = false;
  world.addresses = [{ host: "10.0.0.2", port: 1 }];
  world.catalog = { projects: [], monos: [], sessions: [session("one")] };
  world.store.set(
    "monocode.mobile.pairing",
    JSON.stringify({ fingerprint: "fp", addresses: world.addresses }),
  );
  Object.assign(renders, { command: 0, monoTools: 0, everything: 0 });
  root = createRoot(document.createElement("div"));
  await act(async () =>
    root.render(
      <ConnectionProvider>
        <CommandOnly />
        <ToolsOnly />
        <Everything />
      </ConnectionProvider>,
    ),
  );
  await settle();
  world.writes.length = 0;
});
afterEach(() => {
  act(() => root.unmount());
  vi.useRealTimers();
});

describe("ConnectionProvider polling", () => {
  it("reads every five seconds while the app is active, busy or not", async () => {
    world.requests = 0;
    await advance(5000 * 12 + 100);
    expect(world.requests).toBe(12);
    world.catalog = { projects: [], monos: [], sessions: [session("one", true)] };
    world.requests = 0;
    await advance(5000 * 12);
    expect(world.requests).toBe(12);
  });

  it("shows a session started on the desktop within one poll", async () => {
    const seen = renders.everything;
    world.catalog = { projects: [], monos: [], sessions: [session("one"), session("new")] };
    await advance(5000);
    expect(renders.everything).toBeGreaterThan(seen);
  });

  it("backs off after failures", async () => {
    world.fail = true;
    await advance(5000 * 3);
    world.requests = 0;
    await advance(300_000);
    // A fixed 5 s retry would be 60 reads.
    expect(world.requests).toBeLessThan(15);
  });

  it("does not read in the background", async () => {
    world.appState = "background";
    await advance(5000 * 3);
    world.requests = 0;
    await advance(5000 * 30);
    expect(world.requests).toBe(0);
    world.appState = "active";
    await act(async () => {
      for (const listener of [...world.appListeners]) listener("active");
    });
    await settle();
    expect(world.requests).toBeGreaterThan(0);
  });
});

describe("ConnectionProvider writes and renders", () => {
  it("writes neither the pairing nor the catalog when nothing changed", async () => {
    await advance(30000 * 4);
    expect(pairingWrites()).toBe(0);
    expect(catalogWrites()).toBeLessThanOrEqual(1);
    const before = catalogWrites();
    await advance(30000 * 4);
    expect(catalogWrites()).toBe(before);
  });

  it("writes the pairing when the addresses change", async () => {
    world.addresses = [{ host: "10.0.0.3", port: 1 }];
    await advance(5000 + 100);
    expect(pairingWrites()).toBe(1);
  });

  it("saves a changing catalog at most once per interval, and when backgrounded", async () => {
    let n = 0;
    const change = () => {
      world.catalog = { projects: [], monos: [], sessions: [session("one"), session(`s${++n}`)] };
    };
    // A catalog that changes at every 5 s read, for two minutes.
    world.catalog = { projects: [], monos: [], sessions: [session("one", true)] };
    for (let i = 0; i < 24; i++) {
      change();
      (world.catalog.sessions as { busy: boolean }[])[0].busy = true;
      await advance(5000);
    }
    expect(catalogWrites()).toBeLessThanOrEqual(5);
    expect(catalogWrites()).toBeGreaterThanOrEqual(2);
    change();
    await advance(5000);
    const before = catalogWrites();
    await act(async () => {
      world.appState = "background";
      for (const listener of [...world.appListeners]) listener("background");
    });
    await settle();
    expect(catalogWrites()).toBeGreaterThan(before - 1);
    const saved = JSON.parse(world.store.get("monocode.mobile.fp.catalog") ?? "{}");
    expect(saved.sessions.length).toBe(2);
  });

  it("a poll that finds nothing new updates no state and renders nothing", async () => {
    const before = { ...renders };
    const commits = vi.fn();
    const { Profiler } = await import("react");
    await act(async () =>
      root.render(
        <Profiler id="p" onRender={commits}>
          <ConnectionProvider>
            <CommandOnly />
            <ToolsOnly />
            <Everything />
          </ConnectionProvider>
        </Profiler>,
      ),
    );
    await settle();
    // Let the first read land, then count only the polls after it.
    await advance(5000);
    const quiet = { ...renders };
    commits.mockClear();
    world.requests = 0;
    await advance(5000 * 12);
    expect(world.requests).toBe(12);
    // Twelve reads of the same workspace: no render of any reader, no commit.
    expect(renders).toEqual(quiet);
    expect(commits).not.toHaveBeenCalled();
    expect(before.everything).toBeGreaterThan(0);
  });

  it("re-renders command-only and flag-only readers never, whatever the catalog does", async () => {
    const command = renders.command;
    const tools = renders.monoTools;
    const everything = renders.everything;
    world.catalog = { projects: [], monos: [], sessions: [session("one"), session("two")] };
    await advance(15000 + 100);
    expect(renders.everything).toBeGreaterThan(everything);
    expect(renders.command).toBe(command);
    expect(renders.monoTools).toBe(tools);
  });
});

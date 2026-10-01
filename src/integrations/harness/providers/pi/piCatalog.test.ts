import { afterEach, beforeEach, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  close: vi.fn(),
  killChild: vi.fn(async () => {}),
  request: vi.fn(async () => ({ data: [] })),
  resolveBinary: vi.fn(async () => ({ path: "/fake/pi" })),
  spawnChild: vi.fn(async () => {}),
  unwatchChild: vi.fn(),
  watchChild: vi.fn(),
}));

vi.mock("../../../../platform/tauri/fs", () => ({
  homeDir: vi.fn(async () => "/home/test"),
}));
vi.mock("../../core/child", () => ({
  killChild: mocks.killChild,
  resolveOmpBinary: mocks.resolveBinary,
  resolvePiBinary: mocks.resolveBinary,
  spawnChild: mocks.spawnChild,
  unwatchChild: mocks.unwatchChild,
  watchChild: mocks.watchChild,
}));
vi.mock("./piClient", () => ({
  PiRpc: class {
    close = mocks.close;
    pushLine = vi.fn();
    request = mocks.request;
  },
}));
vi.mock("./piProtocol", () => ({
  buildPiSpawnArgs: vi.fn(() => []),
  modelsFromRpcData: vi.fn(() => []),
}));

import { discoverPiModels, refreshPiCatalog } from "./piCatalog";
import { modelsFromRpcData } from "./piProtocol";
import { hasLiveCatalog, modelsFor, resetHarnessModelOverlays, setHarnessModels } from "../../../../features/sessions/model/models";

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
});

afterEach(() => {
  vi.useRealTimers();
  resetHarnessModelOverlays();
});

it("clears the outer discovery timeout after a successful probe", async () => {
  await discoverPiModels("/workspace");
  expect(vi.getTimerCount()).toBe(0);
  expect(mocks.killChild).toHaveBeenCalled();
});

it("reports failed refresh without discarding a retained live overlay", async () => {
  const old = [{ id: "pi:old", harness: "pi" as const, name: "Retained" }];
  setHarnessModels("pi", old);
  mocks.request.mockRejectedValueOnce(new Error("Probe failed"));
  expect(await refreshPiCatalog()).toEqual({ status: "failed", error: "Probe failed" });
  expect(hasLiveCatalog("pi")).toBe(true);
  expect(modelsFor("pi")).toEqual(old);
  expect(vi.getTimerCount()).toBe(0);
});

it("reports empty discovery as failed rather than claiming live success", async () => {
  expect(await refreshPiCatalog()).toEqual({ status: "failed", error: "Pi catalog returned no models" });
  expect(hasLiveCatalog("pi")).toBe(false);
});

it("reports successful nonempty discovery and shares an in-flight probe", async () => {
  const fresh = [{ id: "pi:fresh", harness: "pi" as const, name: "Fresh" }];
  vi.mocked(modelsFromRpcData).mockReturnValueOnce(fresh);
  const first = refreshPiCatalog();
  const second = refreshPiCatalog();
  expect(first).toBe(second);
  expect(await first).toEqual({ status: "succeeded" });
  expect(modelsFor("pi")).toEqual(fresh);
  expect(mocks.spawnChild).toHaveBeenCalledOnce();
  expect(vi.getTimerCount()).toBe(0);
});

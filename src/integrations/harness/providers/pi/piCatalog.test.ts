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
vi.mock("../../../../features/sessions/model/models", () => ({
  setHarnessModels: vi.fn(),
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

import { discoverPiModels } from "./piCatalog";

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
});

afterEach(() => {
  vi.useRealTimers();
});

it("clears the outer discovery timeout after a successful probe", async () => {
  await discoverPiModels("/workspace");
  expect(vi.getTimerCount()).toBe(0);
  expect(mocks.killChild).toHaveBeenCalled();
});

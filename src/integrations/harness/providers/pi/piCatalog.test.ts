import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  hasLiveCatalog,
  modelsFor,
  resetHarnessModelOverlays,
} from "../../../../features/sessions/model/models";

const mocks = vi.hoisted(() => ({
  close: vi.fn(),
  frames: [] as Array<(record: Record<string, unknown>) => void>,
  killChild: vi.fn(),
  request: vi.fn(),
  resolveBinary: vi.fn(),
  resolveOmpBinary: vi.fn(),
  spawnChild: vi.fn(),
  unwatchChild: vi.fn(),
  watchChild: vi.fn(),
  writeChild: vi.fn(),
}));

vi.mock("../../core/child", () => ({
  killChild: mocks.killChild,
  resolveOmpBinary: mocks.resolveOmpBinary,
  resolvePiBinary: mocks.resolveBinary,
  spawnChild: mocks.spawnChild,
  unwatchChild: mocks.unwatchChild,
  watchChild: mocks.watchChild,
  writeChild: mocks.writeChild,
}));

vi.mock("../../../../platform/tauri/fs", () => ({
  homeDir: vi.fn(async () => "/home/test"),
}));

vi.mock("./piClient", () => ({
  PiRpc: class {
    pushLine = vi.fn();
    request = mocks.request;
    close = mocks.close;

    constructor(
      _sessionId: string,
      onFrame: (record: Record<string, unknown>) => void,
    ) {
      mocks.frames.push(onFrame);
    }
  },
}));

import { refreshOmpCatalog, refreshPiCatalog } from "./piCatalog";

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

const flavors = [
  {
    name: "pi",
    refresh: refreshPiCatalog,
    harness: "pi" as const,
    binary: "/bin/pi",
    probeId: "monocode-pi-probe",
  },
  {
    name: "omp",
    refresh: refreshOmpCatalog,
    harness: "omp" as const,
    binary: "/bin/omp",
    probeId: "monocode-omp-probe",
  },
];

beforeEach(() => {
  resetHarnessModelOverlays();
  for (const value of Object.values(mocks)) {
    if (typeof value === "function" && "mockReset" in value) {
      value.mockReset();
    }
  }
  mocks.frames.length = 0;
  mocks.resolveBinary.mockResolvedValue({ path: "/bin/pi" });
  mocks.resolveOmpBinary.mockResolvedValue({ path: "/bin/omp" });
  mocks.spawnChild.mockResolvedValue(undefined);
  mocks.killChild.mockResolvedValue(undefined);
  mocks.writeChild.mockResolvedValue(undefined);
  mocks.request.mockResolvedValue({ data: { models: [] } });
  mocks.watchChild.mockImplementation(() => undefined);
});

describe.each(flavors)(
  "$name catalog discovery",
  ({ refresh, harness, binary, probeId }) => {
    it("loads extensions without passing --no-extensions", async () => {
      await refresh();

      expect(mocks.spawnChild).toHaveBeenCalledWith(
        probeId,
        binary,
        ["--mode", "rpc", "--no-session"],
        "/home/test",
        undefined,
        harness,
      );
      expect(mocks.spawnChild.mock.calls[0]![2]).not.toContain(
        "--no-extensions",
      );
      expect(mocks.request).toHaveBeenCalledWith(
        { type: "get_available_models" },
        45_000,
      );
      expect(mocks.close).toHaveBeenCalledOnce();
      expect(mocks.unwatchChild).toHaveBeenCalledWith(probeId);
      expect(mocks.killChild).toHaveBeenCalledWith(probeId);
    });

    it("denies extension UI requests that require a reply", async () => {
      const response = deferred<Record<string, unknown>>();
      mocks.request.mockReturnValue(response.promise);
      const discovery = refresh();
      await vi.waitFor(() => expect(mocks.frames).toHaveLength(1));

      mocks.frames[0]!({
        type: "extension_ui_request",
        id: "ui-1",
        method: "confirm",
        title: "Continue?",
      });
      await vi.waitFor(() => expect(mocks.writeChild).toHaveBeenCalledOnce());
      expect(mocks.writeChild).toHaveBeenCalledWith(
        probeId,
        JSON.stringify({
          type: "extension_ui_response",
          id: "ui-1",
          cancelled: true,
        }),
      );

      response.resolve({ data: { models: [] } });
      await discovery;
    });

    it("adds custom-provider models to the picker", async () => {
      mocks.request.mockResolvedValue({
        data: {
          models: [
            {
              id: "openrouter-model",
              name: "OpenRouter Model",
              provider: "my-custom",
            },
          ],
        },
      });

      await refresh();

      expect(modelsFor(harness).map((model) => model.id)).toEqual([
        `${harness}:my-custom/openrouter-model`,
      ]);
      expect(hasLiveCatalog(harness)).toBe(true);
    });

    it("does not replace the picker when discovery returns no models", async () => {
      await refresh();
      expect(hasLiveCatalog(harness)).toBe(false);
    });
  },
);

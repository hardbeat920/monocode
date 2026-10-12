import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { JsonRpcMessage } from "../../core/acp";

const mocks = vi.hoisted(() => ({
  execChild: vi.fn<(...args: unknown[]) => Promise<string>>(),
  killChild: vi.fn(async () => undefined),
  resolveCursorBinary: vi.fn(async () => ({ path: "/fake/cursor-agent" })),
  setHarnessModels: vi.fn(),
  spawnChild: vi.fn<(...args: unknown[]) => Promise<void>>(
    async () => undefined,
  ),
  unwatchChild: vi.fn(),
}));

const sent: JsonRpcMessage[] = [];
let onLine: ((line: string) => void) | undefined;
let responses: Record<string, Pick<JsonRpcMessage, "result" | "error">>;

vi.mock("../../../../platform/tauri/fs", () => ({
  homeDir: async () => "/home/test",
}));
vi.mock("../../../../features/sessions/model/models", () => ({
  setHarnessModels: mocks.setHarnessModels,
}));
vi.mock("../../core/child", () => ({
  ...mocks,
  watchChild: (_id: string, line: (value: string) => void) => {
    onLine = line;
  },
  writeChild: async (_id: string, line: string) => {
    const request = JSON.parse(line) as JsonRpcMessage;
    sent.push(request);
    const response = responses[request.method!];
    if (!response) throw new Error(`Unexpected request: ${request.method}`);
    onLine!(JSON.stringify({ jsonrpc: "2.0", id: request.id, ...response }));
  },
}));

import { discoverCursorModels, refreshCursorCatalog } from "./cursorCatalog";

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  sent.length = 0;
  onLine = undefined;
  responses = {
    initialize: {
      result: { authMethods: [{ id: "cursor_login", name: "Cursor Login" }] },
    },
    "cursor/list_available_models": {
      result: {
        models: [{ value: "composer-test", name: "Composer Test" }],
      },
    },
  };
  mocks.execChild.mockReset().mockResolvedValue("");
  vi.spyOn(console, "debug").mockImplementation(() => undefined);
});

afterEach(() => {
  expect(vi.getTimerCount()).toBe(0);
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("Cursor catalog discovery", () => {
  it("refreshes models using existing CLI credentials without requesting browser sign-in", async () => {
    await refreshCursorCatalog();

    expect(mocks.setHarnessModels).toHaveBeenCalledWith("cursor", [
      {
        id: "cursor:composer-test",
        harness: "cursor",
        name: "Composer Test",
        nativeId: "composer-test",
        settings: undefined,
      },
    ]);
    expect(sent.map((request) => request.method)).toEqual([
      "initialize",
      "cursor/list_available_models",
    ]);
    expect(mocks.execChild).not.toHaveBeenCalled();
    expect(mocks.spawnChild).toHaveBeenCalledWith(
      expect.any(String),
      "/fake/cursor-agent",
      ["acp"],
      "/home/test",
      undefined,
      "cursor",
    );
    const probeId = mocks.spawnChild.mock.calls[0][0];
    expect(mocks.unwatchChild).toHaveBeenCalledWith(probeId);
    expect(mocks.killChild).toHaveBeenCalledWith(probeId);
  });

  it("still discovers models from session setup without an authentication request", async () => {
    responses["cursor/list_available_models"] = { result: { models: [] } };
    responses["session/new"] = {
      result: {
        models: {
          availableModels: [{ modelId: "gpt-test", name: "GPT Test" }],
        },
      },
    };

    expect(await discoverCursorModels("/workspace")).toEqual([
      {
        id: "cursor:gpt-test",
        harness: "cursor",
        name: "GPT Test",
        nativeId: "gpt-test",
      },
    ]);
    expect(sent.map((request) => request.method)).toEqual([
      "initialize",
      "cursor/list_available_models",
      "session/new",
    ]);
    expect(sent.at(-1)?.params).toEqual({ cwd: "/workspace", mcpServers: [] });
  });

  it("falls back to CLI models when ACP requires login without starting a login flow", async () => {
    responses["cursor/list_available_models"] = {
      error: { code: -32000, message: "Authentication required" },
    };
    mocks.execChild.mockResolvedValue(
      "gpt-test-low - GPT Test Low\ngpt-test-high - GPT Test High\n",
    );

    expect(await discoverCursorModels("/workspace")).toMatchObject([
      {
        id: "cursor:gpt-test",
        name: "GPT Test",
        nativeId: "gpt-test",
        settings: [
          {
            id: "reasoning",
            options: [
              { value: "low", label: "Low" },
              { value: "high", label: "High" },
            ],
          },
        ],
      },
    ]);
    expect(sent.map((request) => request.method)).toEqual([
      "initialize",
      "cursor/list_available_models",
    ]);
    expect(mocks.execChild).toHaveBeenCalledWith(
      "/fake/cursor-agent",
      ["--list-models"],
      "/workspace",
      "cursor",
    );
    expect(mocks.killChild).toHaveBeenCalled();
  });

  it("keeps the bundled catalog when both discovery paths require sign-in", async () => {
    responses["cursor/list_available_models"] = {
      error: { code: -32000, message: "Authentication required" },
    };
    mocks.execChild.mockRejectedValue(new Error("Not authenticated"));

    await refreshCursorCatalog();

    expect(mocks.setHarnessModels).not.toHaveBeenCalled();
    expect(sent.some((request) => request.method === "authenticate")).toBe(
      false,
    );
    expect(mocks.killChild).toHaveBeenCalled();
  });
});

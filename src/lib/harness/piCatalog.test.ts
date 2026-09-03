import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  writeChild: vi.fn(),
  spawnChild: vi.fn(),
  killChild: vi.fn(),
  unwatchChild: vi.fn(),
  setHarnessModels: vi.fn(),
}));

const state = {
  push: (_line: string) => {
    throw new Error("watchChild not registered");
  },
};

vi.mock("./child", () => ({
  spawnChild: mocks.spawnChild,
  killChild: mocks.killChild,
  unwatchChild: mocks.unwatchChild,
  watchChild: (_sessionId: string, onLine: (line: string) => void) => {
    state.push = onLine;
  },
  writeChild: mocks.writeChild,
}));

vi.mock("../fs", () => ({ homeDir: () => Promise.resolve("/tmp") }));

vi.mock("../models", () => ({ setHarnessModels: mocks.setHarnessModels }));

vi.mock("./piFlavor", () => ({
  PI_FLAVOR: {
    id: "pi",
    label: "Pi",
    resolveBinary: () => Promise.resolve({ path: "/fake/pi" }),
    resumeFlag: "--session",
    isolateFlags: [],
    probeChildId: "monocode-pi-probe",
    textChildId: "monocode-pi-text",
  },
  OMP_FLAVOR: {
    id: "omp",
    label: "omp",
    resolveBinary: () => Promise.resolve({ path: "/fake/omp" }),
    resumeFlag: "--resume",
    isolateFlags: [],
    probeChildId: "monocode-omp-probe",
    textChildId: "monocode-omp-text",
  },
}));

import { refreshOmpCatalog } from "./piCatalog";
import { asRecord } from "./piProtocol";

const CHUNK_BYTES = 256 * 1024;

function chunkify(payload: Record<string, unknown>): string[] {
  const bytes = new TextEncoder().encode(JSON.stringify(payload));
  const byteLength = bytes.byteLength;
  const count = Math.ceil(byteLength / CHUNK_BYTES);
  const lines: string[] = [];
  for (let index = 0; index < count; index++) {
    const slice = bytes.subarray(index * CHUNK_BYTES, (index + 1) * CHUNK_BYTES);
    let binary = "";
    for (const byte of slice) binary += String.fromCharCode(byte);
    lines.push(
      JSON.stringify({
        type: "rpc_chunk",
        chunkId: "rpc-1",
        index,
        count,
        byteLength,
        data: btoa(binary),
      }),
    );
  }
  return lines;
}

function commandOf(line: string): Record<string, unknown> {
  const rec = asRecord(JSON.parse(line));
  if (!rec) throw new Error("test sent a non-object command");
  return rec;
}

const CATALOG = [
  { id: "a", provider: "openrouter", name: "Model A" },
  { id: "b", provider: "openrouter", name: "Model B" },
  { id: "c", provider: "opencode-zen", name: "Model C" },
];

beforeEach(() => {
  mocks.writeChild.mockReset();
  mocks.spawnChild.mockReset();
  mocks.killChild.mockReset();
  mocks.unwatchChild.mockReset();
  mocks.setHarnessModels.mockReset();
  mocks.spawnChild.mockResolvedValue(undefined);
  mocks.killChild.mockResolvedValue(undefined);
  state.push = () => {
    throw new Error("watchChild not registered");
  };
});

describe("refreshOmpCatalog", () => {
  it("negotiates v2 and discovers a chunked catalog", async () => {
    mocks.writeChild.mockImplementation(
      async (_sessionId: string, line: string) => {
        const cmd = commandOf(line);
        if (cmd.type === "negotiate_protocol") {
          state.push(
            JSON.stringify({
              id: cmd.id,
              type: "response",
              command: "negotiate_protocol",
              success: true,
              data: { protocolVersion: 2 },
            }),
          );
          return;
        }
        const payload = {
          id: cmd.id,
          type: "response",
          command: "get_available_models",
          success: true,
          data: { models: CATALOG, pad: "x".repeat(1_200_000) },
        };
        for (const chunk of chunkify(payload)) state.push(chunk);
      },
    );

    await refreshOmpCatalog();

    expect(mocks.setHarnessModels).toHaveBeenCalledTimes(1);
    const [harness, models] = mocks.setHarnessModels.mock.calls[0] as [
      string,
      Array<{ id: string }>,
    ];
    expect(harness).toBe("omp");
    expect(models.map((model) => model.id).sort()).toEqual([
      "omp:opencode-zen/c",
      "omp:openrouter/a",
      "omp:openrouter/b",
    ]);

    const first = commandOf(mocks.writeChild.mock.calls[0][1] as string);
    expect(first.type).toBe("negotiate_protocol");
    expect(first.protocolVersion).toBe(2);
  });

  it("falls back to v1 when negotiate fails", async () => {
    mocks.writeChild.mockImplementation(
      async (_sessionId: string, line: string) => {
        const cmd = commandOf(line);
        if (cmd.type === "negotiate_protocol") {
          state.push(
            JSON.stringify({
              id: cmd.id,
              type: "response",
              command: "negotiate_protocol",
              success: false,
              error: "unknown command",
            }),
          );
          return;
        }
        state.push(
          JSON.stringify({
            id: cmd.id,
            type: "response",
            command: "get_available_models",
            success: true,
            data: { models: CATALOG.slice(0, 2) },
          }),
        );
      },
    );

    await refreshOmpCatalog();

    expect(mocks.setHarnessModels).toHaveBeenCalledTimes(1);
    const [harness, models] = mocks.setHarnessModels.mock.calls[0] as [
      string,
      Array<{ id: string }>,
    ];
    expect(harness).toBe("omp");
    expect(models.map((model) => model.id).sort()).toEqual([
      "omp:openrouter/a",
      "omp:openrouter/b",
    ]);
  });
});

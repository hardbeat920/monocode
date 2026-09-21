import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Attachment } from "../../../../features/sessions/model/session";

const acpState = vi.hoisted(() => ({ failCompact: false }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args: unknown) => invokeMock(command, args),
}));

vi.mock("../../core/child", () => ({
  resolveOpenCrabsBinary: vi
    .fn()
    .mockResolvedValue({ path: "/fake/opencrabs" }),
  spawnChild: vi.fn().mockResolvedValue(undefined),
  killChild: vi.fn().mockResolvedValue(undefined),
  watchChild: vi.fn(),
  unwatchChild: vi.fn(),
}));

vi.mock("../../core/acp", () => {
  class FakeAcp {
    constructor(_sessionId: string, _handlers: Record<string, unknown>) {}
    request(method: string) {
      if (method === "session/compact" && acpState.failCompact) {
        return Promise.reject(new Error("Request timed out"));
      }
      switch (method) {
        case "initialize":
          return Promise.resolve({ protocolVersion: 1 });
        case "session/new":
          return Promise.resolve({
            sessionId: "acp-1",
            models: { currentModelId: null, availableModels: [] },
          });
        default:
          return Promise.resolve({});
      }
    }
    notify() {
      return Promise.resolve();
    }
    respond() {
      return Promise.resolve();
    }
    respondError() {
      return Promise.resolve();
    }
    rejectPending() {}
    pushLine() {}
    close() {}
  }
  return { AcpClient: FakeAcp };
});

const invokeMock = vi.fn();

import { compactOpenCrabsContext } from "./opencrabs";
import { killChild, spawnChild } from "../../core/child";

function compactInput() {
  return {
    sessionId: "t-compact",
    cwd: "/workspace",
    model: "opencrabs:",
    runtimeMode: "supervised",
    intent: "ask",
    onEvent: () => undefined,
    attachments: [] as Attachment[],
  };
}

describe("compactOpenCrabsContext", () => {
  beforeEach(() => {
    acpState.failCompact = false;
    vi.clearAllMocks();
  });

  it("recycles the transport when compact fails, so the next call respawns", async () => {
    // 1. First compact on a cold session: spawns, session/new, compact OK.
    await expect(compactOpenCrabsContext(compactInput())).resolves.toBeUndefined();

    // 2. Same session + cwd reuses the live transport; its compact times
    //    out — the failure must surface, not swallow.
    acpState.failCompact = true;
    await expect(compactOpenCrabsContext(compactInput())).rejects.toThrow(
      "Request timed out",
    );

    // 3. The wedged transport was recycled: child killed, and the next
    //    compact spawns a fresh child instead of reusing the dead one.
    expect(killChild).toHaveBeenCalledWith("t-compact");
    const spawnsBefore = vi.mocked(spawnChild).mock.calls.length;

    acpState.failCompact = false;
    await expect(compactOpenCrabsContext(compactInput())).resolves.toBeUndefined();
    expect(vi.mocked(spawnChild).mock.calls.length).toBe(spawnsBefore + 1);
  });
});

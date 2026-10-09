// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { newSession, type Session } from "../../sessions/model/session";
import {
  createKeepAwakeController,
  isWorkingSession,
  useKeepAwake,
} from "./keepAwake";
import { saveKeepAwakeEnabled } from "./settings";

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  invoke: vi.fn(async () => undefined),
  isTauri: vi.fn(() => true),
}));

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockResolvedValue(undefined);
  vi.mocked(isTauri).mockReturnValue(true);
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

const session = (id: string, busy = false) => ({
  ...newSession("claude", "/repo"),
  id,
  busy,
});

describe("keep awake", () => {
  it("counts only actively working sessions across providers", () => {
    const completed = session("completed");
    const queued = { ...session("queued"), queueStatus: "paused" as const };
    const waiting = {
      ...session("waiting", true),
      pendingQuestion: { requestId: 1, questions: [] },
    };
    const removed = { ...session("removed", true), worktreeRemoved: true };
    expect(isWorkingSession(completed)).toBe(false);
    expect(isWorkingSession(queued)).toBe(false);
    expect(isWorkingSession(waiting)).toBe(false);
    expect(isWorkingSession(removed)).toBe(false);
    expect(
      isWorkingSession({
        ...session("codex", true),
        harness: "codex" as const,
      }),
    ).toBe(true);
  });

  it("holds the request until the last concurrent agent finishes", async () => {
    const send = vi.fn(async (_enabled: boolean) => undefined);
    const controller = createKeepAwakeController(send);
    const first = session("first", true);
    const second = { ...session("second", true), harness: "codex" as const };

    controller.update(true, [first, second]);
    controller.update(true, [{ ...first, busy: false }, second]);
    await controller.settled();
    expect(send).toHaveBeenCalledTimes(1);
    expect(send).toHaveBeenCalledWith(true, false);

    controller.update(true, [
      { ...first, busy: false },
      { ...second, busy: false },
    ]);
    await controller.settled();
    expect(send).toHaveBeenLastCalledWith(false, false);
  });

  it("releases on setting off or window teardown in IPC order", async () => {
    const transitions: boolean[] = [];
    const controller = createKeepAwakeController(async (enabled) => {
      await Promise.resolve();
      transitions.push(enabled);
    });
    const working = session("working", true);
    controller.update(true, [working]);
    controller.update(false, [working]);
    controller.update(true, [working]);
    controller.release();
    controller.update(true, [working]);
    await controller.settled();
    expect(transitions).toEqual([true, false, true, false]);
  });

  it("continues after a rejected IPC request", async () => {
    const send = vi
      .fn()
      .mockRejectedValueOnce(new Error("unavailable"))
      .mockResolvedValue(undefined);
    const controller = createKeepAwakeController(send);
    controller.update(true, [session("working", true)]);
    controller.release();
    await controller.settled();
    expect(send.mock.calls).toEqual([
      [true, false],
      [false, false],
    ]);
  });

  it("retries a failed enable while the agent is still working", async () => {
    const send = vi
      .fn()
      .mockRejectedValueOnce(new Error("unavailable"))
      .mockResolvedValue(undefined);
    const controller = createKeepAwakeController(send);

    controller.update(true, [session("working", true)]);
    await controller.settled();
    expect(send.mock.calls).toEqual([
      [true, false],
      [true, false],
    ]);
  });

  it("allows a later update to retry after both enable attempts fail", async () => {
    const send = vi
      .fn()
      .mockRejectedValueOnce(new Error("unavailable"))
      .mockRejectedValueOnce(new Error("still unavailable"))
      .mockResolvedValue(undefined);
    const controller = createKeepAwakeController(send);
    const working = session("working", true);

    controller.update(true, [working]);
    await controller.settled();
    expect(send.mock.calls).toEqual([
      [true, false],
      [true, false],
    ]);

    controller.update(true, [working]);
    await controller.settled();
    expect(send.mock.calls).toEqual([
      [true, false],
      [true, false],
      [true, false],
    ]);
  });

  it("does not invoke native IPC in a browser preview", async () => {
    vi.mocked(isTauri).mockReturnValue(false);
    saveKeepAwakeEnabled(true);
    const container = document.createElement("div");
    const root = createRoot(container);
    const Harness = ({ sessions }: { sessions: Session[] }) => {
      useKeepAwake(sessions);
      return null;
    };
    await act(async () => {
      root.render(
        createElement(Harness, { sessions: [session("active", true)] }),
      );
    });
    await act(async () => root.unmount());
    expect(invoke).not.toHaveBeenCalled();
  });

  it("retries a rejected native invoke while work remains active", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("power request failed"));
    saveKeepAwakeEnabled(true);
    const container = document.createElement("div");
    const root = createRoot(container);
    const Harness = ({ sessions }: { sessions: Session[] }) => {
      useKeepAwake(sessions);
      return null;
    };

    await act(async () => {
      root.render(
        createElement(Harness, { sessions: [session("active", true)] }),
      );
    });
    await vi.waitFor(() =>
      expect(
        vi
          .mocked(invoke)
          .mock.calls.filter(
            ([command, args]) =>
              command === "set_keep_awake" && args?.enabled === true,
          ),
      ).toHaveLength(2),
    );
    await act(async () => root.unmount());
  });

  it("holds the request after the last agent for the chosen duration", async () => {
    vi.useFakeTimers();
    const send = vi.fn(async (_enabled: boolean) => undefined);
    const controller = createKeepAwakeController(send);
    const working = session("working", true);

    controller.update(true, [working], 15 * 60 * 1000);
    await controller.settled();
    controller.update(true, [{ ...working, busy: false }], 15 * 60 * 1000);
    await controller.settled();
    expect(send).toHaveBeenLastCalledWith(true, false);

    await vi.advanceTimersByTimeAsync(15 * 60 * 1000 - 1);
    await controller.settled();
    expect(send).toHaveBeenLastCalledWith(true, false);

    await vi.advanceTimersByTimeAsync(1);
    await controller.settled();
    expect(send).toHaveBeenLastCalledWith(false, false);
  });

  it("keeps the request until the setting is turned off when hold is forever", async () => {
    const send = vi.fn(async (_enabled: boolean) => undefined);
    const controller = createKeepAwakeController(send);
    const working = session("working", true);

    controller.update(true, [working], Number.POSITIVE_INFINITY);
    controller.update(
      true,
      [{ ...working, busy: false }],
      Number.POSITIVE_INFINITY,
    );
    await controller.settled();
    expect(send.mock.calls).toEqual([[true, false]]);

    controller.update(
      false,
      [{ ...working, busy: false }],
      Number.POSITIVE_INFINITY,
    );
    await controller.settled();
    expect(send).toHaveBeenLastCalledWith(false, false);
  });

  it("cancels a hold when another agent starts", async () => {
    vi.useFakeTimers();
    const send = vi.fn(async (_enabled: boolean) => undefined);
    const controller = createKeepAwakeController(send);
    const first = session("first", true);

    controller.update(true, [first], 15 * 60 * 1000);
    controller.update(true, [{ ...first, busy: false }], 15 * 60 * 1000);
    await controller.settled();
    const second = session("second", true);
    controller.update(true, [second], 15 * 60 * 1000);
    await vi.advanceTimersByTimeAsync(15 * 60 * 1000);
    await controller.settled();
    expect(send).toHaveBeenLastCalledWith(true, false);
  });

  it("asks native to keep the display on when that setting is on", async () => {
    const send = vi.fn(
      async (_enabled: boolean, _display?: boolean) => undefined,
    );
    const controller = createKeepAwakeController(send);
    const working = session("working", true);

    controller.update(true, [working], 0, true);
    await controller.settled();
    expect(send).toHaveBeenLastCalledWith(true, true);

    controller.update(true, [working], 0, false);
    await controller.settled();
    expect(send).toHaveBeenLastCalledWith(true, false);
  });

  it("reports this window's activity and releases it on pagehide", async () => {
    saveKeepAwakeEnabled(true);
    const container = document.createElement("div");
    const root = createRoot(container);
    const Harness = ({ sessions }: { sessions: Session[] }) => {
      useKeepAwake(sessions);
      return null;
    };
    await act(async () => {
      root.render(
        createElement(Harness, { sessions: [session("active", true)] }),
      );
    });
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("set_keep_awake", {
        enabled: true,
        display: false,
      }),
    );
    window.dispatchEvent(new Event("pagehide"));
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenLastCalledWith("set_keep_awake", {
        enabled: false,
        display: false,
      }),
    );
    await act(async () => root.unmount());
  });
});

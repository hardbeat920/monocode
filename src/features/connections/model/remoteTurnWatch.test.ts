import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { newSession, type Block } from "../../sessions/model/session";
import type { HostSession } from "./protocol";
import {
  pruneRemoteTurnWatches,
  remoteTurnOutcome,
  watchRemoteTurn,
} from "./remoteTurnWatch";

function snapshot(
  blocks: Block[],
  status: HostSession["status"] = "idle",
): HostSession {
  return {
    projectId: "project",
    revision: 1,
    status,
    updatedAt: 1,
    session: {
      ...newSession("codex", "/home/me/repo"),
      id: "host-session",
      busy: status === "running",
      blocks,
    },
  };
}
const asked: Block = { id: "command", role: "user", text: "Fix it" };

describe("remoteTurnOutcome", () => {
  it("waits while the host runs the turn or has not shown it yet", () => {
    expect(remoteTurnOutcome(snapshot([asked], "running"), "command")).toBe(
      undefined,
    );
    expect(remoteTurnOutcome(snapshot([]), "command")).toBe(undefined);
  });

  it("reports the turn's last reply, ignoring earlier turns and tools", () => {
    expect(
      remoteTurnOutcome(
        snapshot([
          { id: "old", role: "assistant", text: "Earlier" },
          asked,
          { id: "a", role: "assistant", text: "Looking" },
          { id: "t", role: "assistant", text: "ls", tool: { name: "bash" } } as Block,
          { id: "b", role: "assistant", text: "Fixed" },
          { id: "s", role: "system", text: "Compacting", statusKey: "compact" },
        ]),
        "command",
      ),
    ).toEqual({ status: "completed", text: "Fixed" });
  });

  it("tells stopped and failed turns apart from the host's closing message", () => {
    expect(
      remoteTurnOutcome(
        snapshot([asked, { id: "s", role: "system", text: "Stopped by you." }]),
        "command",
      ),
    ).toEqual({ status: "cancelled", text: "" });
    expect(
      remoteTurnOutcome(
        snapshot([
          asked,
          { id: "a", role: "assistant", text: "Partial" },
          { id: "s", role: "system", text: "Provider crashed" },
        ]),
        "command",
      ),
    ).toEqual({ status: "failed", text: "Partial", error: "Provider crashed" });
    expect(
      remoteTurnOutcome(snapshot([asked], "interrupted"), "command"),
    ).toMatchObject({ status: "failed" });
  });
});

describe("watchRemoteTurn", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("follows the host from busy to idle and settles once", async () => {
    const reads = [
      snapshot([asked], "running"),
      snapshot([asked, { id: "a", role: "assistant", text: "Wor" }], "running"),
      snapshot([asked, { id: "a", role: "assistant", text: "Done" }]),
    ];
    const load = vi.fn(async () => reads.shift() ?? reads[0]);
    const onSnapshot = vi.fn();
    const onSettled = vi.fn();
    watchRemoteTurn("shell", { commandId: "command", load, onSnapshot, onSettled });
    await vi.advanceTimersByTimeAsync(0);
    expect(onSettled).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1_500);
    expect(onSettled).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1_500);
    expect(onSettled).toHaveBeenCalledExactlyOnceWith({
      status: "completed",
      text: "Done",
    });
    expect(onSnapshot).toHaveBeenCalledTimes(3);
    // Each read after the first is incremental from the last one.
    expect(load.mock.calls[1][0]).toMatchObject({ status: "running" });
    await vi.advanceTimersByTimeAsync(30_000);
    expect(load).toHaveBeenCalledTimes(3);
    pruneRemoteTurnWatches(() => false);
    expect(onSettled).toHaveBeenCalledOnce();
  });

  it("keeps waiting through connection errors", async () => {
    const load = vi
      .fn<() => Promise<HostSession>>()
      .mockRejectedValueOnce(new Error("SSH dropped"))
      .mockResolvedValue(snapshot([asked]));
    const onSettled = vi.fn();
    watchRemoteTurn("shell", { commandId: "command", load, onSettled });
    await vi.advanceTimersByTimeAsync(0);
    expect(onSettled).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1_500);
    expect(onSettled).toHaveBeenCalledWith({ status: "completed", text: "" });
  });

  it("fails a turn the host never shows instead of waiting forever", async () => {
    const onSettled = vi.fn();
    watchRemoteTurn("shell", {
      commandId: "command",
      load: async () => snapshot([]),
      onSettled,
    });
    await vi.advanceTimersByTimeAsync(3_000);
    expect(onSettled).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({ status: "failed" }),
    );
  });

  it("ends only the closed tab's watch and stops reading", async () => {
    const load = vi.fn(async () => snapshot([asked], "running"));
    const closed = vi.fn();
    const open = vi.fn();
    watchRemoteTurn("closed", { commandId: "command", load, onSettled: closed });
    watchRemoteTurn("open", { commandId: "command", load, onSettled: open });
    await vi.advanceTimersByTimeAsync(0);
    pruneRemoteTurnWatches((shellId) => shellId === "open");
    expect(closed).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({ status: "cancelled" }),
    );
    expect(open).not.toHaveBeenCalled();
    load.mockClear();
    await vi.advanceTimersByTimeAsync(1_500);
    expect(load).toHaveBeenCalledOnce();
    pruneRemoteTurnWatches(() => false);
    expect(open).toHaveBeenCalledOnce();
  });
});

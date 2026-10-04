import { describe, expect, it, vi } from "vitest";
import { newSession } from "../../sessions/model/session";
import {
  archiveOperatorSession,
  type OperatorArchiveAdapter,
} from "./operatorSessionArchive";

function adapter() {
  const live = newSession("codex", "/tmp/project");
  live.id = "target";
  let archived = false;
  const value: OperatorArchiveAdapter = {
    callerId: "caller",
    liveSession: vi.fn(() => live),
    isBusy: vi.fn((id) => id === "target" && !!live.busy),
    persistedSession: vi.fn(async () => ({ archived })),
    archiveView: vi.fn(async () => {
      const changed = !archived;
      archived = true;
      return { completed: true, changed };
    }),
    setArchived: vi.fn(async (_id, next) => {
      const changed = archived !== next;
      archived = next;
      return changed;
    }),
  };
  return { value, live, isArchived: () => archived };
}

describe("Operator session archive adapter", () => {
  it("uses the view lifecycle and reports state changes idempotently", async () => {
    const f = adapter();
    await expect(
      archiveOperatorSession("target", true, f.value),
    ).resolves.toEqual({ archived: true, changed: true });
    await expect(
      archiveOperatorSession("target", true, f.value),
    ).resolves.toEqual({ archived: true, changed: false });
    expect(f.value.archiveView).toHaveBeenCalledTimes(2);
    expect(f.value.setArchived).not.toHaveBeenCalled();
    expect(f.isArchived()).toBe(true);
  });

  it("rejects an unsaved blank tab before closing or persisting it", async () => {
    const f = adapter();
    vi.mocked(f.value.persistedSession).mockResolvedValue(undefined);
    f.live.blocks = [];
    await expect(
      archiveOperatorSession("target", true, f.value),
    ).rejects.toThrow("empty session");
    expect(f.value.archiveView).not.toHaveBeenCalled();
    expect(f.value.setArchived).not.toHaveBeenCalled();
  });

  it("persists an unsaved conversation through the archive view lifecycle", async () => {
    const f = adapter();
    vi.mocked(f.value.persistedSession).mockResolvedValue(undefined);
    f.live.blocks = [{ id: "u1", role: "user", text: "Keep this" }];
    await expect(
      archiveOperatorSession("target", true, f.value),
    ).resolves.toEqual({ archived: true, changed: true });
    expect(f.value.archiveView).toHaveBeenCalledOnce();
  });

  it("checks live busy state after the persisted lookup and leaves unarchive available", async () => {
    const f = adapter();
    vi.mocked(f.value.persistedSession).mockImplementation(async () => {
      f.live.busy = true;
      return { archived: false };
    });
    await expect(
      archiveOperatorSession("target", true, f.value),
    ).rejects.toThrow("busy");
    expect(f.value.archiveView).not.toHaveBeenCalled();

    await expect(
      archiveOperatorSession("target", false, f.value),
    ).resolves.toEqual({ archived: false, changed: false });
    expect(f.value.setArchived).toHaveBeenCalledWith("target", false);
  });

  it("rejects the caller and missing sessions", async () => {
    const f = adapter();
    await expect(
      archiveOperatorSession("caller", true, f.value),
    ).rejects.toThrow("current session");
    vi.mocked(f.value.persistedSession).mockResolvedValue(undefined);
    vi.mocked(f.value.liveSession).mockReturnValue(undefined);
    await expect(
      archiveOperatorSession("missing", true, f.value),
    ).rejects.toThrow("not found in this project");
  });

  it("does not claim success when the user declines the close safeguards", async () => {
    const f = adapter();
    vi.mocked(f.value.archiveView).mockResolvedValue({
      completed: false,
      changed: false,
    });
    await expect(
      archiveOperatorSession("target", true, f.value),
    ).rejects.toThrow("cancelled");
  });

  it("propagates a failed unarchive write instead of returning success", async () => {
    const f = adapter();
    vi.mocked(f.value.setArchived).mockRejectedValue(
      new Error("session store unavailable"),
    );
    await expect(
      archiveOperatorSession("target", false, f.value),
    ).rejects.toThrow("session store unavailable");
  });
});

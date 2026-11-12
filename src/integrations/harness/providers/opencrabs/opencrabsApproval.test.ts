import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { handlePermission, type ApprovalTarget } from "./opencrabsApproval";
import type { AcpClient } from "../../core/acp";
import type { HarnessEvent } from "../../core/types";
import type { RuntimeMode } from "../../../../features/sessions/model/session";

const permissionParams = {
  sessionId: "s-1",
  toolCall: {
    toolCallId: "call-1",
    title: "bash",
    kind: "execute",
    rawInput: { command: "ls" },
  },
  options: [{ optionId: "allow-once" }, { optionId: "reject-once" }],
};

function makeTarget(): {
  target: ApprovalTarget;
  respond: ReturnType<typeof vi.fn>;
  events: HarnessEvent[];
} {
  const respond = vi.fn().mockResolvedValue(undefined);
  const events: HarnessEvent[] = [];
  const target: ApprovalTarget = {
    acp: { respond } as unknown as AcpClient,
    planning: false,
    runtimeMode: "supervised" as RuntimeMode,
    onEvent: (event) => events.push(event),
    approvals: new Map(),
  };
  return { target, respond, events };
}

describe("handlePermission dialog expiry", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("expires the dialog to deny just under the server's 300s deadline", async () => {
    const { target, respond, events } = makeTarget();
    const pending = handlePermission(target, 7, permissionParams);
    await vi.advanceTimersByTimeAsync(0);
    expect(events.some((e) => e.type === "approval.requested")).toBe(true);

    await vi.advanceTimersByTimeAsync(290_000);
    await pending;

    const resolved = events[events.length - 1];
    expect(resolved).toMatchObject({
      type: "approval.resolved",
      requestId: 7,
      decision: "deny",
    });
    expect(respond).toHaveBeenCalledTimes(1);
    expect(respond).toHaveBeenCalledWith(7, {
      outcome: { outcome: "selected", optionId: "reject-once" },
    });
    expect(target.approvals.size).toBe(0);

    // The settled timer is cleared: no second resolution long after expiry.
    await vi.advanceTimersByTimeAsync(600_000);
    expect(respond).toHaveBeenCalledTimes(1);
    expect(events.filter((e) => e.type === "approval.resolved")).toHaveLength(1);
  });

  it("a user answer before expiry cancels the timer", async () => {
    const { target, respond, events } = makeTarget();
    const pending = handlePermission(target, 8, permissionParams);
    await vi.advanceTimersByTimeAsync(0);

    target.approvals.get(8)?.("allow");
    await pending;

    expect(respond).toHaveBeenCalledTimes(1);
    expect(respond).toHaveBeenCalledWith(8, {
      outcome: { outcome: "selected", optionId: "allow-once" },
    });

    // Long past the deadline the cleared timer never fires a stale deny.
    await vi.advanceTimersByTimeAsync(600_000);
    expect(respond).toHaveBeenCalledTimes(1);
    const resolved = events.filter((e) => e.type === "approval.resolved");
    expect(resolved).toHaveLength(1);
    expect(resolved[0]).toMatchObject({ decision: "allow" });
  });
});

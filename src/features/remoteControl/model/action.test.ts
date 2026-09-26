import { describe, expect, it } from "vitest";
import { remoteControlAction, type RemoteControlTarget } from "./action";
import { HARNESSES } from "../../sessions/model/session";

function target(over: Partial<RemoteControlTarget> = {}): RemoteControlTarget {
  return { harness: "claude", providerSessionId: "sess-1", active: false, ...over };
}

describe("remoteControlAction", () => {
  it("offers the action for a running Claude session", () => {
    expect(remoteControlAction(target())).toEqual({
      id: "remote-control",
      label: "Open Remote Control",
      intent: "open",
      disabled: false,
    });
  });

  it.each(HARNESSES.filter((harness) => harness !== "claude"))(
    "is absent for %s rather than disabled",
    (harness) => {
      expect(remoteControlAction(target({ harness }))).toBeNull();
    },
  );

  it("covers every harness the app knows about", () => {
    // Guards against a new harness silently inheriting the Claude-only path.
    const offered = HARNESSES.filter(
      (harness) => remoteControlAction(target({ harness })) !== null,
    );
    expect(offered).toEqual(["claude"]);
  });

  it("offers close once remote control is active", () => {
    const action = remoteControlAction(target({ active: true }));
    expect(action).toEqual({
      id: "remote-control",
      label: "Close Remote Control",
      intent: "close",
      disabled: false,
    });
  });

  it("stays visible but disabled when no session id is bound yet", () => {
    const action = remoteControlAction(
      target({ providerSessionId: undefined }),
    );
    expect(action).not.toBeNull();
    expect(action?.disabled).toBe(true);
    expect(action?.intent).toBe("open");
    expect(action?.description).toMatch(/send a message first/i);
  });

  it.each(["", undefined] as const)(
    "treats %j as no bound session",
    (providerSessionId) => {
      expect(remoteControlAction(target({ providerSessionId }))?.disabled).toBe(
        true,
      );
    },
  );

  it("can still close an active session with no id bound", () => {
    // Closing acts on a process that is already running, so a missing id — a
    // rebound thread, a restarted child — must not strand it open.
    const action = remoteControlAction(
      target({ active: true, providerSessionId: undefined }),
    );
    expect(action?.disabled).toBe(false);
    expect(action?.intent).toBe("close");
  });

  it("never describes an action the user can actually take", () => {
    for (const active of [false, true]) {
      for (const providerSessionId of ["sess-1", undefined]) {
        const action = remoteControlAction(
          target({ active, providerSessionId }),
        );
        if (action && !action.disabled) {
          expect(action.description).toBeUndefined();
        }
      }
    }
  });

  it("keeps label and intent in step", () => {
    for (const active of [false, true]) {
      const action = remoteControlAction(target({ active }));
      expect(action?.label.toLowerCase()).toContain(action?.intent ?? "");
    }
  });
});

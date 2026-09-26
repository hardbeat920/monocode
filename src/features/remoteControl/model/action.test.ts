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

  // The reported confusion: a brand-new session under `all` mode, where nothing
  // was wrong and nothing was required, read as a blocker the user had to clear.
  it("says an unbound thread opens by itself when the mode is automatic", () => {
    const action = remoteControlAction(
      target({ providerSessionId: undefined, automatic: true }),
    );
    expect(action?.disabled).toBe(true);
    expect(action?.intent).toBe("open");
    expect(action?.description).toBe("Opens by itself once the first turn ends");
    expect(action?.description).not.toMatch(/send a message/i);
  });

  it("still asks for a first message when nothing is automatic", () => {
    expect(
      remoteControlAction(target({ providerSessionId: undefined }))?.description,
    ).toMatch(/send a message first/i);
  });

  // Saying so is what stops the menu inviting a second click for an instruction
  // already given, and names the reason it is waiting rather than refusing.
  it("says a queued hand-over is waiting for the turn", () => {
    const action = remoteControlAction(target({ queued: true }));
    expect(action?.disabled).toBe(true);
    expect(action?.intent).toBe("open");
    expect(action?.description).toMatch(/opens when this turn ends/i);
    expect(action?.description).toMatch(/not interrupted/i);
  });

  it("prefers the queue to the missing id, since the queue is the newer fact", () => {
    expect(
      remoteControlAction(
        target({ providerSessionId: undefined, queued: true, automatic: true }),
      )?.description,
    ).toMatch(/opens when this turn ends/i);
  });

  // Closing acts on a process that is already running, so nothing about waiting
  // to open may disable it.
  it.each([
    ["queued", { queued: true }],
    ["automatic", { automatic: true }],
    ["both", { queued: true, automatic: true }],
  ] as const)("can still close while %s", (_name, over) => {
    const action = remoteControlAction(target({ active: true, ...over }));
    expect(action?.disabled).toBe(false);
    expect(action?.intent).toBe("close");
    expect(action?.label).toBe("Close Remote Control");
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
        for (const automatic of [false, true]) {
          for (const queued of [false, true]) {
            const action = remoteControlAction(
              target({ active, providerSessionId, automatic, queued }),
            );
            if (action && !action.disabled) {
              expect(action.description).toBeUndefined();
            }
          }
        }
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

import { describe, expect, it } from "vitest";
import {
  noteInterruptedTurn,
  remoteControlName,
  remoteControlStep,
  remoteControlTarget,
  seatRemoteUserMessage,
  shouldAutoOpen,
} from "./remoteControlSession";
import { newSession, type Session } from "../features/sessions/model/session";

function session(patch: Partial<Session> = {}): Session {
  return { ...newSession("claude", "/repo/monocode"), ...patch };
}

describe("remote control naming", () => {
  it("names a session after its project", () => {
    expect(remoteControlName("/repo/monocode", [])).toBe("monocode");
  });

  it("distinguishes several threads of one project", () => {
    const first = remoteControlName("/repo/monocode", []);
    const second = remoteControlName("/repo/monocode", [first]);
    const third = remoteControlName("/repo/monocode", [first, second]);

    expect([first, second, third]).toEqual([
      "monocode",
      "monocode 2",
      "monocode 3",
    ]);
  });

  it("resolves against the names in use, not a count", () => {
    // The first thread closed. Its name is free, and the name already given to
    // the surviving process must not be handed out again either.
    expect(remoteControlName("/repo/monocode", ["monocode 2"])).toBe("monocode");
  });

  it("does not collide across projects", () => {
    expect(remoteControlName("/repo/other", ["monocode"])).toBe("other");
  });
});

describe("acting on a menu click", () => {
  it("opens and closes when the menu agreed with the state", () => {
    expect(remoteControlStep("open", false)).toBe("open");
    expect(remoteControlStep("close", true)).toBe("close");
  });

  it.each([
    ["open", true],
    ["close", false],
  ] as const)(
    "does nothing rather than flip when asked to %s against the state",
    (intent, open) => {
      // The click was aimed at something that has since changed. Doing the
      // other thing would carry out an instruction nobody gave.
      expect(remoteControlStep(intent, open)).toBe("none");
    },
  );
});

describe("remote control target", () => {
  it("reports the bound conversation and whether it is live", () => {
    const bound = session({ providerSessionId: "sess-abc" });

    expect(remoteControlTarget(bound, true)).toEqual({
      harness: "claude",
      providerSessionId: "sess-abc",
      active: true,
    });
  });
});

describe("an inbound message from the phone", () => {
  it("is seated as a user turn, not as a system note", () => {
    const next = seatRemoteUserMessage(session(), {
      type: "remote.userMessage",
      text: "  try the other branch  ",
    });

    const block = next.blocks.at(-1);
    // A user block, because that is what it is: a turn on this conversation
    // that did not come from this composer.
    expect(block?.role).toBe("user");
    expect(block?.text).toBe("try the other branch");
    expect(block?.startedAt).toBeTypeOf("number");
  });

  it("ignores an empty message rather than seating a blank turn", () => {
    const before = session();
    const next = seatRemoteUserMessage(before, {
      type: "remote.userMessage",
      text: "   ",
    });

    expect(next).toBe(before);
  });
});

describe("the cue for a turn the handover stopped", () => {
  it("explains the interruption when a turn was running", () => {
    const next = noteInterruptedTurn(session({ busy: true }));

    const block = next.blocks.at(-1);
    expect(block?.role).toBe("system");
    expect(block?.notice).toBe("error");
    expect(block?.text).toMatch(/stopped the turn that was running/);
  });

  it("says nothing when no turn was running", () => {
    const idle = session({ busy: false });

    expect(noteInterruptedTurn(idle)).toBe(idle);
  });
});

describe("all mode opening lazily", () => {
  const ready = session({ providerSessionId: "sess-abc", busy: false });

  it("opens a used, idle Claude thread", () => {
    expect(
      shouldAutoOpen(ready, { mode: "all", open: false, dismissed: false }),
    ).toBe(true);
  });

  it("does nothing in manual mode", () => {
    expect(
      shouldAutoOpen(ready, { mode: "manual", open: false, dismissed: false }),
    ).toBe(false);
  });

  it("waits for a conversation to exist", () => {
    // Before a first turn there is no bound id, so `--resume` would have
    // nothing to open. This is what makes the mode lazy rather than eager.
    expect(
      shouldAutoOpen(session({ providerSessionId: undefined }), {
        mode: "all",
        open: false,
        dismissed: false,
      }),
    ).toBe(false);
  });

  it("never interrupts a running turn", () => {
    // Opening stops the headless child, so auto-opening mid-turn would kill the
    // turn the user is waiting on without them asking for anything.
    expect(
      shouldAutoOpen({ ...ready, busy: true }, {
        mode: "all",
        open: false,
        dismissed: false,
      }),
    ).toBe(false);
  });

  it("does not reopen what the user closed", () => {
    expect(
      shouldAutoOpen(ready, { mode: "all", open: false, dismissed: true }),
    ).toBe(false);
  });

  it("does not open a second time", () => {
    expect(
      shouldAutoOpen(ready, { mode: "all", open: true, dismissed: false }),
    ).toBe(false);
  });

  it("leaves other harnesses alone", () => {
    expect(
      shouldAutoOpen(
        { ...newSession("codex", "/repo/monocode"), providerSessionId: "x" },
        { mode: "all", open: false, dismissed: false },
      ),
    ).toBe(false);
  });
});

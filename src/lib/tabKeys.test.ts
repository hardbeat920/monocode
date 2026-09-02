import { describe, expect, it } from "vitest";
import {
  deferUnhandledEscape,
  focusedBusyAgentSessionId,
  shouldStopFocusedTurnOnEscape,
  tabCommand,
} from "./tabKeys";

function key(
  partial: Partial<
    Pick<
      KeyboardEvent,
      | "key"
      | "code"
      | "metaKey"
      | "ctrlKey"
      | "altKey"
      | "shiftKey"
      | "isComposing"
      | "defaultPrevented"
      | "repeat"
    >
  >,
): KeyboardEvent {
  return {
    isComposing: false,
    defaultPrevented: false,
    repeat: false,
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    key: "",
    code: "",
    ...partial,
  } as KeyboardEvent;
}

describe("tabCommand", () => {
  it("opens a terminal pane with cmd-backtick", () => {
    expect(tabCommand(key({ key: "`", code: "Backquote", metaKey: true }))).toBe(
      "new-terminal",
    );
  });

  it("opens a terminal workspace tab with shift-cmd-backtick", () => {
    expect(
      tabCommand(
        key({ key: "~", code: "Backquote", metaKey: true, shiftKey: true }),
      ),
    ).toBe("new-terminal-tab");
  });

  it("keeps existing tab chrome bindings", () => {
    expect(tabCommand(key({ key: "t", metaKey: true }))).toBe("new");
    expect(
      tabCommand(key({ key: "t", metaKey: true, altKey: true })),
    ).toBe("close-others");
    expect(
      tabCommand(key({ key: "t", ctrlKey: true, altKey: true })),
    ).toBe("close-others");
    expect(tabCommand(key({ key: "d", metaKey: true }))).toBe("split-right");
    expect(tabCommand(key({ key: "j", metaKey: true }))).toBe(
      "toggle-terminal",
    );
  });

  it("walks tab visit history with cmd-brackets", () => {
    expect(
      tabCommand(key({ key: "[", code: "BracketLeft", metaKey: true })),
    ).toBe("back");
    expect(
      tabCommand(key({ key: "]", code: "BracketRight", metaKey: true })),
    ).toBe("forward");
  });

  it("keeps shift-cmd-brackets as adjacent tab cycle", () => {
    expect(
      tabCommand(
        key({ key: "{", code: "BracketLeft", metaKey: true, shiftKey: true }),
      ),
    ).toBe("prev");
    expect(
      tabCommand(
        key({ key: "}", code: "BracketRight", metaKey: true, shiftKey: true }),
      ),
    ).toBe("next");
  });
});

describe("shouldStopFocusedTurnOnEscape", () => {
  const escape = (partial: Partial<KeyboardEvent> = {}) =>
    key({ key: "Escape", ...partial });

  it("stops a busy focused agent turn on plain Escape", () => {
    expect(
      shouldStopFocusedTurnOnEscape(escape(), {
        inTerminal: false,
        focusedSessionBusy: true,
      }),
    ).toBe(true);
  });

  it("does not steal Escape that another surface already handled", () => {
    expect(
      shouldStopFocusedTurnOnEscape(escape({ defaultPrevented: true }), {
        inTerminal: false,
        focusedSessionBusy: true,
      }),
    ).toBe(false);
  });

  it("leaves terminal Escape and idle sessions alone", () => {
    expect(
      shouldStopFocusedTurnOnEscape(escape(), {
        inTerminal: true,
        focusedSessionBusy: true,
      }),
    ).toBe(false);
    expect(
      shouldStopFocusedTurnOnEscape(escape(), {
        inTerminal: false,
        focusedSessionBusy: false,
      }),
    ).toBe(false);
  });

  it("ignores modified, composing, or repeated Escape", () => {
    for (const partial of [
      { metaKey: true },
      { ctrlKey: true },
      { altKey: true },
      { shiftKey: true },
      { isComposing: true },
      { repeat: true },
    ]) {
      expect(
        shouldStopFocusedTurnOnEscape(escape(partial), {
          inTerminal: false,
          focusedSessionBusy: true,
        }),
      ).toBe(false);
    }
  });
});

describe("focusedBusyAgentSessionId", () => {
  const tabs = [{ id: "tab-a", focusedId: "session-a" }];
  const sessions = [{ id: "session-a", busy: true }];

  it("returns only the busy agent session in the exact active tab", () => {
    expect(focusedBusyAgentSessionId("tab-a", tabs, sessions, false)).toBe(
      "session-a",
    );
    expect(
      focusedBusyAgentSessionId("missing", tabs, sessions, false),
    ).toBeNull();
  });

  it("does not stop through diff, terminal-dock, editor, or idle focus", () => {
    expect(
      focusedBusyAgentSessionId(
        "tab-a",
        [{ ...tabs[0], diffFocused: true }],
        sessions,
        false,
      ),
    ).toBeNull();
    expect(focusedBusyAgentSessionId("tab-a", tabs, sessions, true)).toBeNull();
    expect(
      focusedBusyAgentSessionId(
        "tab-a",
        [{ id: "tab-a", focusedId: "editor-pane" }],
        sessions,
        false,
      ),
    ).toBeNull();
    expect(
      focusedBusyAgentSessionId(
        "tab-a",
        tabs,
        [{ id: "session-a", busy: false }],
        false,
      ),
    ).toBeNull();
  });
});

describe("deferUnhandledEscape", () => {
  const escape = (partial: Partial<KeyboardEvent> = {}) =>
    key({ key: "Escape", ...partial });

  it("runs after the keydown dispatch when Escape stays unhandled", () => {
    let deferred: (() => void) | undefined;
    let stopped = false;
    deferUnhandledEscape(
      escape(),
      () => {
        stopped = true;
      },
      (callback) => {
        deferred = callback;
      },
    );
    expect(stopped).toBe(false);
    deferred?.();
    expect(stopped).toBe(true);
  });

  it("yields to a later same-dispatch Escape handler", () => {
    let deferred: (() => void) | undefined;
    let stopped = false;
    const event = escape() as KeyboardEvent & { defaultPrevented: boolean };
    deferUnhandledEscape(
      event,
      () => {
        stopped = true;
      },
      (callback) => {
        deferred = callback;
      },
    );
    Object.defineProperty(event, "defaultPrevented", { value: true });
    deferred?.();
    expect(stopped).toBe(false);
  });

  it("does not schedule an already-handled or repeated Escape", () => {
    let scheduled = 0;
    const defer = () => {
      scheduled += 1;
    };
    deferUnhandledEscape(escape({ defaultPrevented: true }), () => {}, defer);
    deferUnhandledEscape(escape({ repeat: true }), () => {}, defer);
    expect(scheduled).toBe(0);
  });
});

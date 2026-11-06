// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import {
  adjacentItemId,
  deferUnhandledEscape,
  focusedBusyAgentSessionId,
  hasBlockingProjectNavigationSurface,
  hasBlockingListNavigationSurface,
  shouldHandleListNavigation,
  shouldStopFocusedTurnOnEscape,
  tabCommand,
  tabCommandForKeybinding,
  tabCommandKeybinding,
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
  it("archives with Cmd+Shift+A or Ctrl+Shift+A", () => {
    expect(tabCommand(key({ key: "A", metaKey: true, shiftKey: true }))).toBe(
      "archive-session",
    );
    expect(tabCommand(key({ key: "a", ctrlKey: true, shiftKey: true }))).toBe(
      "archive-session",
    );
  });

  it.each([
    {},
    { metaKey: true },
    { ctrlKey: true },
    { shiftKey: true },
    { metaKey: true, shiftKey: true, altKey: true },
    { metaKey: true, shiftKey: true, isComposing: true },
    { metaKey: true, shiftKey: true, repeat: true },
  ])("leaves other A key events alone (%j)", (modifiers) => {
    expect(tabCommand(key({ key: "a", ...modifiers }))).toBeNull();
  });

  it("opens a terminal pane with cmd-backtick", () => {
    expect(
      tabCommand(key({ key: "`", code: "Backquote", metaKey: true })),
    ).toBe("new-terminal");
  });

  it("opens a terminal workspace tab with shift-cmd-backtick", () => {
    expect(
      tabCommand(
        key({ key: "~", code: "Backquote", metaKey: true, shiftKey: true }),
      ),
    ).toBe("new-terminal-tab");
  });

  it("closes all tabs with Cmd+Shift+W or Ctrl+Shift+W", () => {
    expect(tabCommand(key({ key: "W", metaKey: true, shiftKey: true }))).toBe(
      "close-all",
    );
    expect(tabCommand(key({ key: "w", ctrlKey: true, shiftKey: true }))).toBe(
      "close-all",
    );
    expect(tabCommand(key({ key: "w", metaKey: true }))).toBe("close");
  });

  it("keeps Ctrl+Tab separate from the adjacent tab shortcut", () => {
    expect(tabCommand(key({ key: "Tab", ctrlKey: true }))).toBe("cycle-next");
    expect(tabCommand(key({ key: "Tab", ctrlKey: true, shiftKey: true }))).toBe(
      "cycle-prev",
    );
  });

  it("keeps existing tab chrome bindings", () => {
    expect(tabCommand(key({ key: "t", metaKey: true }))).toBe("new");
    expect(tabCommand(key({ key: "t", metaKey: true, altKey: true }))).toBe(
      "close-others",
    );
    expect(tabCommand(key({ key: "t", ctrlKey: true, altKey: true }))).toBe(
      "close-others",
    );
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

  it("uses shift-mod arrows for session and project navigation", () => {
    expect(
      tabCommand(key({ key: "ArrowUp", metaKey: true, shiftKey: true })),
    ).toBe("prev-session");
    expect(
      tabCommand(key({ key: "ArrowDown", metaKey: true, shiftKey: true })),
    ).toBe("next-session");
    expect(
      tabCommand(key({ key: "ArrowLeft", metaKey: true, shiftKey: true })),
    ).toBe("prev-project");
    expect(
      tabCommand(key({ key: "ArrowRight", metaKey: true, shiftKey: true })),
    ).toBe("next-project");
  });

  it("uses unshifted mod arrows to switch sessions in the current tab", () => {
    expect(tabCommand(key({ key: "ArrowUp", metaKey: true }))).toBe(
      "prev-session-in-tab",
    );
    expect(tabCommand(key({ key: "ArrowDown", ctrlKey: true }))).toBe(
      "next-session-in-tab",
    );
    expect(
      tabCommand(
        key({ key: "ArrowUp", metaKey: true, altKey: true, shiftKey: true }),
      ),
    ).toBeNull();
  });

  it("maps parsed commands to configurable keybinding rows", () => {
    expect(tabCommandKeybinding("new")).toBe("Tab: New");
    expect(tabCommandKeybinding("cycle-next")).toBe("Tab: Cycle Next");
    expect(tabCommandKeybinding({ activate: 0 })).toBe("Tab: Activate 1–8");
    expect(tabCommandKeybinding({ activate: -1 })).toBe("Tab: Activate Last");
    expect(tabCommandKeybinding({ focus: "left" })).toBe("Pane: Focus Left");
  });

  it("maps custom keybinding rows back to commands", () => {
    expect(tabCommandForKeybinding("Tab: New", key({ key: "y" }))).toBe("new");
    expect(
      tabCommandForKeybinding("Tab: Activate 1–8", key({ code: "Digit3" })),
    ).toEqual({ activate: 2 });
    expect(
      tabCommandForKeybinding("Pane: Focus Down", key({ key: "ArrowDown" })),
    ).toEqual({ focus: "down" });
  });

  it("cycles ordered item ids and wraps at both ends", () => {
    expect(adjacentItemId(["a", "b", "c"], "b", 1)).toBe("c");
    expect(adjacentItemId(["a", "b", "c"], "c", 1)).toBe("a");
    expect(adjacentItemId(["a", "b", "c"], "a", -1)).toBe("c");
    expect(adjacentItemId(["a", "b", "c"], "missing", 1)).toBe("a");
    expect(adjacentItemId(["a", "b", "c"], "missing", -1)).toBe("c");
    expect(adjacentItemId([], "a", 1)).toBeNull();
  });

  it("allows navigation from an empty composer", () => {
    expect(
      shouldHandleListNavigation({
        blockedTarget: true,
        emptyComposerTarget: true,
        surfaceOpen: false,
      }),
    ).toBe(true);
  });

  it("allows only project navigation from an empty Mono composer", () => {
    const monoComposer = {
      blockedTarget: true,
      emptyComposerTarget: false,
      monoComposerEmptyTarget: true,
      surfaceOpen: false,
    };
    for (const [keyName, expected] of [
      ["ArrowLeft", "prev-project"],
      ["ArrowRight", "next-project"],
    ] as const) {
      const command = tabCommand(
        key({ key: keyName, metaKey: true, shiftKey: true }),
      );
      expect(command).toBe(expected);
      expect(
        shouldHandleListNavigation({
          ...monoComposer,
          projectNavigation:
            command === "prev-project" || command === "next-project",
        }),
      ).toBe(true);
    }
    expect(
      tabCommand(key({ key: "ArrowRight", ctrlKey: true, shiftKey: true })),
    ).toBe("next-project");
    expect(
      shouldHandleListNavigation({ ...monoComposer, projectNavigation: false }),
    ).toBe(false);
    expect(
      shouldHandleListNavigation({
        ...monoComposer,
        projectNavigation: true,
        surfaceOpen: true,
      }),
    ).toBe(false);
  });

  it("blocks list navigation while another text or app surface owns focus", () => {
    expect(
      shouldHandleListNavigation({
        blockedTarget: false,
        emptyComposerTarget: false,
        surfaceOpen: false,
      }),
    ).toBe(true);
    expect(
      shouldHandleListNavigation({
        blockedTarget: true,
        emptyComposerTarget: false,
        surfaceOpen: false,
      }),
    ).toBe(false);
    expect(
      shouldHandleListNavigation({
        blockedTarget: false,
        emptyComposerTarget: false,
        surfaceOpen: true,
      }),
    ).toBe(false);
    expect(
      shouldHandleListNavigation({
        blockedTarget: true,
        emptyComposerTarget: true,
        surfaceOpen: true,
      }),
    ).toBe(false);
  });

  it("blocks project navigation after focus leaves a portalled emoji picker", () => {
    const picker = document.createElement("div");
    picker.dataset.emojiPicker = "";
    const toggle = document.createElement("button");
    const composer = document.createElement("textarea");
    composer.dataset.monoComposerEmpty = "true";
    document.body.append(picker);
    document.body.append(toggle, composer);

    for (const target of [toggle, composer]) {
      expect(hasBlockingProjectNavigationSurface(target)).toBe(true);
      expect(
        shouldHandleListNavigation({
          blockedTarget: target === composer,
          emptyComposerTarget: false,
          monoComposerEmptyTarget: target === composer,
          projectNavigation: true,
          projectNavigationSurfaceOpen:
            hasBlockingProjectNavigationSurface(target),
          surfaceOpen: false,
        }),
      ).toBe(false);
    }
    expect(
      shouldHandleListNavigation({
        blockedTarget: false,
        emptyComposerTarget: false,
        projectNavigation: false,
        projectNavigationSurfaceOpen: true,
        surfaceOpen: hasBlockingListNavigationSurface(toggle),
      }),
    ).toBe(true);

    picker.remove();
    toggle.remove();
    composer.remove();
  });

  it("blocks project navigation from a pending composer attachment button", () => {
    const composer = document.createElement("div");
    composer.dataset.monoComposerPending = "true";
    const button = document.createElement("button");
    composer.append(button);
    document.body.append(composer);

    expect(hasBlockingProjectNavigationSurface(button)).toBe(true);
    expect(
      shouldHandleListNavigation({
        blockedTarget: false,
        emptyComposerTarget: false,
        monoComposerEmptyTarget: false,
        projectNavigation: true,
        projectNavigationSurfaceOpen:
          hasBlockingProjectNavigationSurface(button),
        surfaceOpen: false,
      }),
    ).toBe(false);
    composer.remove();
  });

  it("blocks project navigation while an active Mono read is pending after focus leaves it", () => {
    const composer = document.createElement("div");
    composer.dataset.monoComposerPending = "true";
    const outside = document.createElement("button");
    document.body.append(composer, outside);

    const projectNavigationSurfaceOpen =
      hasBlockingProjectNavigationSurface(outside);
    expect(projectNavigationSurfaceOpen).toBe(true);
    expect(
      shouldHandleListNavigation({
        blockedTarget: false,
        emptyComposerTarget: false,
        projectNavigation: true,
        projectNavigationSurfaceOpen,
        surfaceOpen: false,
      }),
    ).toBe(false);
    expect(
      shouldHandleListNavigation({
        blockedTarget: false,
        emptyComposerTarget: false,
        projectNavigation: false,
        projectNavigationSurfaceOpen,
        surfaceOpen: false,
      }),
    ).toBe(true);

    composer.remove();
    outside.remove();
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

  it("waits for later keydown handlers before stopping the session", async () => {
    vi.useFakeTimers();
    try {
      const event = escape() as KeyboardEvent & { defaultPrevented: boolean };
      const stop = vi.fn();
      deferUnhandledEscape(event, stop);

      // Browsers can flush microtasks between listeners on the same target.
      await Promise.resolve();
      expect(stop).not.toHaveBeenCalled();

      Object.defineProperty(event, "defaultPrevented", { value: true });
      vi.runAllTimers();
      expect(stop).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

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

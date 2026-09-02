/**
 * Workspace keybindings:
 *   New tab             cmd-t
 *   Close other tabs    cmd-opt-t
 *   Close tab           cmd-w
 *   Split pane right    cmd-d
 *   Split pane down     shift-cmd-d
 *   Next tab            shift-cmd-}
 *   Previous tab        shift-cmd-{
 *   Back in tab history cmd-[
 *   Forward in history  cmd-]
 *   Activate tab 1–8    cmd-1 … cmd-8
 *   Last tab            cmd-9
 *   Cycle next tab      ctrl-tab
 *   Cycle previous tab  ctrl-shift-tab
 *   Focus pane          cmd-opt-arrows
 *   New terminal        cmd-`
 *   New terminal tab    shift-cmd-`
 *   Toggle terminal     cmd-j
 *   Stop focused turn   escape
 */

import type { FocusDir } from "./layout";

export type TabCommand =
  | "new"
  | "close-others"
  | "close"
  | "next"
  | "prev"
  | "back"
  | "forward"
  | "split-right"
  | "split-down"
  | "new-terminal"
  | "new-terminal-tab"
  | "toggle-terminal"
  | { activate: number }
  | { focus: FocusDir };

export function tabCommand(e: KeyboardEvent): TabCommand | null {
  if (e.isComposing) return null;

  const mod = e.metaKey || e.ctrlKey;

  if (mod && e.altKey && !e.shiftKey) {
    if (e.key.toLowerCase() === "t") return "close-others";
    if (e.key === "ArrowLeft") return { focus: "left" };
    if (e.key === "ArrowRight") return { focus: "right" };
    if (e.key === "ArrowUp") return { focus: "up" };
    if (e.key === "ArrowDown") return { focus: "down" };
    return null;
  }

  if (e.key === "Tab" && e.ctrlKey && !e.metaKey && !e.altKey) {
    return e.shiftKey ? "prev" : "next";
  }

  if (!mod || e.altKey) return null;

  if (e.key === "`" || e.code === "Backquote") {
    return e.shiftKey ? "new-terminal-tab" : "new-terminal";
  }

  const key = e.key.toLowerCase();

  if (e.shiftKey) {
    if (e.key === "]" || e.key === "}") return "next";
    if (e.key === "[" || e.key === "{") return "prev";
    if (key === "d") return "split-down";
    return null;
  }

  if (key === "t") return "new";
  if (key === "w") return "close";
  if (key === "d") return "split-right";
  if (key === "j") return "toggle-terminal";
  if (e.key === "[" || e.code === "BracketLeft") return "back";
  if (e.key === "]" || e.code === "BracketRight") return "forward";
  if (key >= "1" && key <= "8") return { activate: Number(key) - 1 };
  if (key === "9") return { activate: -1 };
  return null;
}

type EscapeKeyEvent = Pick<
  KeyboardEvent,
  | "key"
  | "isComposing"
  | "defaultPrevented"
  | "repeat"
  | "metaKey"
  | "ctrlKey"
  | "altKey"
  | "shiftKey"
>;

type EscapeFocusTab = {
  id: string;
  focusedId: string;
  diffFocused?: boolean;
};

type EscapeFocusSession = {
  id: string;
  busy?: boolean;
};

function isPlainEscape(e: EscapeKeyEvent): boolean {
  return (
    e.key === "Escape" &&
    !e.isComposing &&
    !e.repeat &&
    !e.metaKey &&
    !e.ctrlKey &&
    !e.altKey &&
    !e.shiftKey
  );
}

export function shouldStopFocusedTurnOnEscape(
  e: EscapeKeyEvent,
  options: { inTerminal: boolean; focusedSessionBusy: boolean },
): boolean {
  return (
    isPlainEscape(e) &&
    !e.defaultPrevented &&
    !options.inTerminal &&
    options.focusedSessionBusy
  );
}

export function focusedBusyAgentSessionId(
  activeTabId: string,
  tabs: readonly EscapeFocusTab[],
  sessions: readonly EscapeFocusSession[],
  projectTerminalFocused: boolean,
): string | null {
  if (projectTerminalFocused) return null;
  const tab = tabs.find((entry) => entry.id === activeTabId);
  if (!tab || tab.diffFocused) return null;
  const session = sessions.find((entry) => entry.id === tab.focusedId);
  return session?.busy === true ? session.id : null;
}

export function deferUnhandledEscape(
  e: EscapeKeyEvent,
  run: () => void,
  defer: (callback: () => void) => void = queueMicrotask,
): void {
  if (!isPlainEscape(e) || e.defaultPrevented) return;
  defer(() => {
    if (!e.defaultPrevented) run();
  });
}

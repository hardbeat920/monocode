import { tabCommand } from "./tabKeys";

type ArchiveContext = {
  activeTabId: string;
  tabs: readonly { id: string; focusedId: string; diffFocused?: boolean }[];
  sessions: readonly { id: string }[];
  projectTerminalFocused: boolean;
  surfaceOpen: boolean;
};

// Popover covers menus, listboxes, toolbars, and role-less flyouts. The other
// selectors cover dialogs and inline composer pickers outside that component.
const OVERLAY_SELECTOR =
  '[data-popover-side], [role="dialog"], [role="alertdialog"], [role="menu"], [data-model-picker], [data-file-picker], [data-branch-picker], [data-skill-picker], [data-mention-picker], [data-app-search]';

function isOpenOverlay(element: Element): boolean {
  if (element.closest('[hidden], [inert], [aria-hidden="true"]')) return false;
  for (
    let ancestor: Element | null = element;
    ancestor;
    ancestor = ancestor.parentElement
  ) {
    const style = getComputedStyle(ancestor);
    if (style.display === "none" || style.visibility === "hidden") return false;
  }
  return true;
}

/** Resolve eligibility before consuming the key, then archive that exact session. */
export function handleArchiveShortcut(
  event: KeyboardEvent,
  context: ArchiveContext,
  archive: (sessionId: string) => void,
): boolean {
  if (
    tabCommand(event) !== "archive-session" ||
    event.defaultPrevented ||
    context.surfaceOpen ||
    context.projectTerminalFocused
  )
    return false;

  const tab = context.tabs.find((entry) => entry.id === context.activeTabId);
  if (!tab || tab.diffFocused) return false;
  const session = context.sessions.find((entry) => entry.id === tab.focusedId);
  if (!session) return false;

  const target = event.target instanceof Element ? event.target : null;
  if (target?.closest(".cm-editor, .monocode-terminal")) return false;
  if (
    target?.closest('input, textarea, select, [contenteditable="true"]') &&
    !target.closest("[data-composer]")
  )
    return false;

  // Some menus leave focus in the composer, so checking target.closest alone
  // would still archive the conversation behind an open popover.
  if (
    Array.from(document.querySelectorAll(OVERLAY_SELECTOR)).some(isOpenOverlay)
  ) {
    return false;
  }

  event.preventDefault();
  event.stopPropagation();
  archive(session.id);
  return true;
}

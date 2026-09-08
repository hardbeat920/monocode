type ArchiveContext = {
  activeTabId: string;
  tabs: readonly { id: string; focusedId: string; diffFocused?: boolean }[];
  sessions: readonly { id: string }[];
  projectTerminalFocused: boolean;
  surfaceOpen: boolean;
};

/** Archive only after confirming that the focused conversation owns the key. */
export function archiveFocusedSession(
  event: KeyboardEvent | undefined,
  context: ArchiveContext,
  archive: (sessionId: string) => void,
): void {
  if (
    event?.defaultPrevented ||
    (!event && !document.hasFocus()) ||
    context.projectTerminalFocused ||
    context.surfaceOpen
  )
    return;

  const tab = context.tabs.find((entry) => entry.id === context.activeTabId);
  if (!tab || tab.diffFocused) return;
  const session = context.sessions.find((entry) => entry.id === tab.focusedId);
  if (!session) return;

  const eventTarget = event?.target ?? document.activeElement;
  const target = eventTarget instanceof Element ? eventTarget : null;
  if (target?.closest(".cm-editor, .monocode-terminal")) return;
  if (
    target?.closest('input, textarea, select, [contenteditable="true"]') &&
    !target.closest("[data-composer]")
  )
    return;

  // Popovers can leave focus in the composer. Check the whole document,
  // excluding overlays in hidden or inactive surfaces.
  const overlayOpen = Array.from(
    document.querySelectorAll(
      '[data-popover-side], [role="dialog"], [role="alertdialog"], [role="menu"], [data-skill-picker], [data-mention-picker]',
    ),
  ).some(
    (element) =>
      element.getClientRects().length > 0 &&
      getComputedStyle(element).visibility !== "hidden" &&
      !element.closest('[hidden], [inert], [aria-hidden="true"]'),
  );
  if (overlayOpen) return;

  event?.preventDefault();
  event?.stopPropagation();
  archive(session.id);
}

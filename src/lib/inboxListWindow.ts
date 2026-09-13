/** First paint of the Inbox; more cards mount as the user scrolls. */
export const INBOX_LIST_PAGE = 32;

/**
 * How many Inbox cards to mount. Keep the selected card inside the mounted
 * window so deep links and retained selections remain visible.
 */
export function inboxListWindow(
  total: number,
  requested: number,
  selectedIndex: number,
): number {
  if (total <= 0) return 0;
  const includeSelected = selectedIndex >= 0 ? selectedIndex + 1 : 0;
  return Math.min(total, Math.max(INBOX_LIST_PAGE, requested, includeSelected));
}

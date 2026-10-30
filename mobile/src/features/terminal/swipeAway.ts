/** What a drag on the terminal's sheet starts on, and what the screen shows. */
export type Holds = {
  /** A full-screen program (an editor, a pager) has the screen. */
  full: boolean;
  /** A finger is on the header or the keys. */
  chrome: boolean;
  /** A finger is on the arrow pad. */
  pad: boolean;
};

/**
 * Whether a drag may swipe the sheet away. It may anywhere iOS sees it isn't a
 * scroll, but not in a full-screen program, which scrolls in the page where
 * iOS can't see it, unless it starts on the header or keys; nor on the arrow
 * pad, where dragging types arrows. Each is known before the drag moves far
 * enough to be a swipe, but for a finger on the chrome or pad, only just.
 */
export const canSwipeAway = ({ full, chrome, pad }: Holds) =>
  !pad && (!full || chrome);

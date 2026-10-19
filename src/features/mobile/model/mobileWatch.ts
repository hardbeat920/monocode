import type { MobileTurn } from "./mobileTranscript";

/** What the phone last received for a watch, to send only what changed since. */
export type WatchBaseline = {
  busy: boolean;
  older: boolean;
  order: string[];
  turns: Map<string, string>;
};

/**
 * The next frame for a watched conversation: every turn's ID in order, plus
 * the turns that changed. While a reply streams that is the one live turn,
 * not the whole transcript. Null when nothing changed.
 */
export function watchUpdate(
  baseline: WatchBaseline | null,
  conversation: { busy: boolean; older: boolean; turns: MobileTurn[] },
): { update: object; baseline: WatchBaseline } | null {
  const turns = new Map(
    conversation.turns.map((turn) => [turn.id, JSON.stringify(turn)]),
  );
  const order = conversation.turns.map((turn) => turn.id);
  const changed = conversation.turns.filter(
    (turn) => baseline?.turns.get(turn.id) !== turns.get(turn.id),
  );
  if (
    baseline &&
    baseline.busy === conversation.busy &&
    baseline.older === conversation.older &&
    changed.length === 0 &&
    baseline.order.length === order.length &&
    baseline.order.every((id, index) => id === order[index])
  )
    return null;
  const { busy, older } = conversation;
  return {
    update: { busy, older, order, turns: changed },
    baseline: { busy, older, order, turns },
  };
}

/**
 * Where a watch's turns start: at turn `from` while it is still there, so the
 * window only grows as the conversation does; otherwise `tail` from the end.
 * `older` says whether there is history before it for the phone to page in.
 */
export function watchWindow(
  turnIds: string[],
  from: string | null,
  tail: number,
): { start: number; from: string | null; older: boolean } {
  let start = from ? turnIds.indexOf(from) : -1;
  if (start < 0) start = Math.max(0, turnIds.length - tail);
  return { start, from: turnIds[start] ?? null, older: start > 0 };
}

/** Up to `limit` turns just before turn `before`, for paging in history. */
export function turnsBefore(
  turnIds: string[],
  before: string,
  limit: number,
): { start: number; end: number; older: boolean } {
  const end = turnIds.indexOf(before);
  if (end < 0) throw new Error("This part of the conversation is no longer available");
  const start = Math.max(0, end - limit);
  return { start, end, older: start > 0 };
}

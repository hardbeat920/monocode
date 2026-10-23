import type { MobileTurn } from "./mobileTranscript";

/** What the phone last received for a watch, to send only what changed since. */
export type WatchBaseline = {
  busy: boolean;
  older: boolean;
  order: string[];
  turns: Map<string, string>;
};

/** Paths use JSON array indexes; append offsets count UTF-16 code units. */
export type WatchEdit = {
  path: (string | number)[];
  value?: unknown;
  append?: string;
  from?: number;
  remove?: true;
};
export type WatchPatch = { id: string; edits: WatchEdit[] };

function editsBetween(
  before: unknown,
  after: unknown,
  path: WatchEdit["path"] = [],
): WatchEdit[] {
  if (before === after) return [];
  if (
    typeof before === "string" &&
    typeof after === "string" &&
    after.startsWith(before)
  )
    return [{ path, from: before.length, append: after.slice(before.length) }];
  if (Array.isArray(before) && Array.isArray(after)) {
    const edits = after.flatMap((value, i) =>
      editsBetween(before[i], value, [...path, i]),
    );
    for (let i = before.length - 1; i >= after.length; i--)
      edits.push({ path: [...path, i], remove: true });
    return edits;
  }
  if (
    before &&
    after &&
    typeof before === "object" &&
    typeof after === "object" &&
    !Array.isArray(before) &&
    !Array.isArray(after)
  ) {
    const old = before as Record<string, unknown>;
    const next = after as Record<string, unknown>;
    return [
      ...Object.keys(old)
        .filter((key) => !(key in next))
        .map((key): WatchEdit => ({ path: [...path, key], remove: true })),
      ...Object.entries(next).flatMap(([key, value]) =>
        editsBetween(old[key], value, [...path, key]),
      ),
    ];
  }
  return [{ path, value: after }];
}

/**
 * The next frame for a watched conversation: every turn's ID in order, plus
 * the turns that changed. While a reply streams that is the one live turn,
 * not the whole transcript. Capable phones opt into field edits and text
 * suffixes instead of retransmitting a growing turn. Null when nothing changed.
 */
export function watchUpdate(
  baseline: WatchBaseline | null,
  conversation: { busy: boolean; older: boolean; turns: MobileTurn[] },
  incremental = false,
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
  const patches: WatchPatch[] = [];
  const replaced = incremental
    ? changed.filter((turn) => {
        const previous = baseline?.turns.get(turn.id);
        if (!previous) return true;
        const patch = {
          id: turn.id,
          edits: editsBetween(
            JSON.parse(previous),
            JSON.parse(turns.get(turn.id)!),
          ),
        };
        // Small items/replacements are cheaper as a snapshot; long text sends its suffix.
        if (JSON.stringify(patch).length >= turns.get(turn.id)!.length)
          return true;
        patches.push(patch);
        return false;
      })
    : changed;
  return {
    update: {
      busy,
      older,
      order,
      turns: replaced,
      ...(patches.length ? { patches } : {}),
    },
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
  if (end < 0)
    throw new Error("This part of the conversation is no longer available");
  const start = Math.max(0, end - limit);
  return { start, end, older: start > 0 };
}

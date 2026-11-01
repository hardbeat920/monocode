/** Where a tap on a notification goes: an inbox item's sheet, by its key. */
export type InboxTarget = { pathname: "/inbox/item"; params: { key: string } };

/**
 * The route a pushed notification opens, if it is the Inbox's: its data is
 * `{kind: "inbox", key}`. Anything else (a session's, a Mono's) is for the
 * other routes.
 */
export function inboxTarget(data: unknown): InboxTarget | null {
  if (!data || typeof data !== "object") return null;
  const { kind, key } = data as { kind?: unknown; key?: unknown };
  if (kind !== "inbox" || typeof key !== "string" || key.length === 0) return null;
  return { pathname: "/inbox/item", params: { key } };
}

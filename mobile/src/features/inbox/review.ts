import { useSyncExternalStore } from "react";

import type { DiffLine } from "@/features/workspace/types";

import type { InboxClient, InboxReviewComment, InboxReviewLine } from "./protocol";

const NONE: InboxReviewComment[] = [];

/** Where a comment on this diff line goes, or nothing for a line that isn't one. */
export function reviewLine(path: string, line: DiffLine): InboxReviewLine | null {
  if (line.kind === "hunk") return null;
  const left = line.kind === "del";
  const number = left ? line.oldNumber : line.newNumber;
  return number === null ? null : { path, line: number, side: left ? "left" : "right" };
}

export function sameLine(a: InboxReviewLine, b: InboxReviewLine) {
  return a.path === b.path && a.line === b.line && a.side === b.side;
}

/** "retry.rs:42", the file name and line, as the desktop labels a comment. */
export function lineLabel({ path, line }: Pick<InboxReviewLine, "path" | "line">) {
  return `${path.slice(path.lastIndexOf("/") + 1)}:${line}`;
}

export function countComments(count: number) {
  return count === 1 ? "1 comment" : `${count} comments`;
}

export type ReviewBook = ReturnType<typeof createReviewBook>;

/**
 * The pending line comments of each pull request, as the desktop holds them.
 * The sheet and the diff screen read one list; every change comes back from
 * the desktop, so a comment written at the desk shows on the next read.
 */
export function createReviewBook(client: InboxClient) {
  const lists = new Map<string, InboxReviewComment[]>();
  const listeners = new Set<() => void>();

  const keep = (key: string, comments: InboxReviewComment[]) => {
    lists.set(key, comments);
    for (const listener of listeners) listener();
  };

  return {
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    comments: (key: string) => lists.get(key) ?? NONE,
    load: async (key: string) => keep(key, await client.review(key)),
    add: async (key: string, line: InboxReviewLine, text: string) =>
      keep(key, await client.reviewAdd(key, line, text)),
    remove: async (key: string, id: number) => keep(key, await client.reviewRemove(key, id)),
    submit: async (key: string) => keep(key, await client.reviewSubmit(key)),
  };
}

export function useReviewComments(book: ReviewBook, key: string) {
  return useSyncExternalStore(book.subscribe, () => book.comments(key));
}

import { describe, expect, it, vi } from "vitest";

import { createFixtureClient } from "@/features/inbox/fixtures";
import { createInboxClient, type InboxTransport } from "@/features/inbox/protocol";
import {
  countComments,
  createReviewBook,
  lineLabel,
  reviewLine,
  sameLine,
} from "@/features/inbox/review";
import type { DiffLine } from "@/features/workspace/types";

const PR = "github:monocode/monocode#1284";
const LINE = { path: "src/net/retry.rs", line: 42, side: "right" } as const;

const row = (kind: DiffLine["kind"], oldNumber: number | null, newNumber: number | null) => ({
  kind,
  text: "code",
  oldNumber,
  newNumber,
});

function fixture() {
  return createFixtureClient({ projectIds: ["p1"], now: () => 0, controls: { latencyMs: 0 } });
}

describe("a diff line's place in a review", () => {
  it("puts a removed line on the left, at its old number", () => {
    expect(reviewLine("a.rs", row("del", 7, null))).toEqual({ path: "a.rs", line: 7, side: "left" });
  });

  it("puts an added or unchanged line on the right, at its new number", () => {
    expect(reviewLine("a.rs", row("add", null, 9))).toEqual({ path: "a.rs", line: 9, side: "right" });
    expect(reviewLine("a.rs", row("context", 7, 9))).toEqual({
      path: "a.rs",
      line: 9,
      side: "right",
    });
  });

  it("gives a hunk header no place", () => {
    expect(reviewLine("a.rs", row("hunk", null, null))).toBeNull();
    expect(reviewLine("a.rs", row("hunk", 7, 9))).toBeNull();
  });

  it("tells the two sides of one number apart", () => {
    expect(sameLine(LINE, { ...LINE })).toBe(true);
    expect(sameLine(LINE, { ...LINE, side: "left" })).toBe(false);
  });

  it("labels a line by its file name", () => {
    expect(lineLabel(LINE)).toBe("retry.rs:42");
    expect(lineLabel({ path: "README.md", line: 1 })).toBe("README.md:1");
    expect(countComments(1)).toBe("1 comment");
    expect(countComments(3)).toBe("3 comments");
  });
});

describe("the review book", () => {
  it("keeps each pull request's list apart and tells its readers", async () => {
    const book = createReviewBook(fixture());
    const reader = vi.fn();
    const stop = book.subscribe(reader);
    await book.add(PR, LINE, "Cap this");
    expect(reader).toHaveBeenCalledTimes(1);
    expect(book.comments(PR)).toMatchObject([{ ...LINE, text: "Cap this" }]);
    expect(book.comments("github:monocode/monocode#1")).toEqual([]);
    stop();
    await book.remove(PR, book.comments(PR)[0].id);
    expect(reader).toHaveBeenCalledTimes(1);
    expect(book.comments(PR)).toEqual([]);
  });

  it("hands out one list until it changes, as useSyncExternalStore needs", async () => {
    const book = createReviewBook(fixture());
    expect(book.comments(PR)).toBe(book.comments(PR));
    await book.add(PR, LINE, "Cap this");
    expect(book.comments(PR)).toBe(book.comments(PR));
  });

  it("reads what the desktop holds", async () => {
    const client = fixture();
    await client.reviewAdd(PR, LINE, "From the desk");
    const book = createReviewBook(client);
    await book.load(PR);
    expect(book.comments(PR)).toMatchObject([{ text: "From the desk" }]);
  });

  it("keeps the list when a post fails", async () => {
    const client = fixture();
    const book = createReviewBook({
      ...client,
      reviewSubmit: () => Promise.reject(new Error("Refused")),
    });
    await book.add(PR, LINE, "Cap this");
    await expect(book.submit(PR)).rejects.toThrow("Refused");
    expect(book.comments(PR)).toHaveLength(1);
  });

  it("keeps what the desktop didn't send", async () => {
    const left = [{ ...LINE, id: 2, text: "Added while posting" }];
    const book = createReviewBook({ ...fixture(), reviewSubmit: async () => left });
    await book.submit(PR);
    expect(book.comments(PR)).toEqual(left);
  });
});

describe("review commands on the link", () => {
  function lossy() {
    const ids: (string | undefined)[] = [];
    const calls: { method: string; params?: object }[] = [];
    const transport: InboxTransport = async <T,>(method: string, params?: object, id?: string) => {
      ids.push(id);
      calls.push({ method, params });
      if (ids.length === 1) throw new Error("The reply was lost");
      return { comments: [] } as T;
    };
    let next = 0;
    return { transport, ids, calls, makeId: () => `id-${++next}` };
  }

  it("retries a line comment with the same id, so it can't be added twice", async () => {
    const link = lossy();
    const client = createInboxClient(link.transport, link.makeId);
    await expect(client.reviewAdd(PR, LINE, "Cap this")).rejects.toThrow("lost");
    await client.reviewAdd(PR, LINE, "Cap this");
    expect(link.ids).toEqual(["id-1", "id-1"]);
    expect(link.calls[0]).toEqual({
      method: "inbox.reviewAdd",
      params: { key: PR, ...LINE, text: "Cap this" },
    });
  });

  it("retries a review with the same id, so GitHub can't get it twice", async () => {
    const link = lossy();
    const client = createInboxClient(link.transport, link.makeId);
    await expect(client.reviewSubmit(PR)).rejects.toThrow("lost");
    await client.reviewSubmit(PR);
    expect(link.ids).toEqual(["id-1", "id-1"]);
  });
});

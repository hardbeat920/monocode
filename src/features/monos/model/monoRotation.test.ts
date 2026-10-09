// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Block } from "../../sessions/model/session";
import {
  loadMonoRotation,
  planRotation,
  rotationReason,
  saveMonoRotation,
} from "./monoRotation";

beforeEach(() => {
  const stored = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
  });
});
afterEach(() => vi.unstubAllGlobals());

const HOUR = 60 * 60 * 1000;
const T0 = new Date(2026, 9, 4, 9).getTime();

let next = 0;
function turn(user: string, reply: string, at: number, extra: Block[] = []) {
  const id = `u${next++}`;
  return [
    { id, role: "user", text: user, startedAt: at, durationMs: 60_000 },
    ...extra,
    { id: `${id}-tool`, role: "assistant", text: "", tool: { title: "Read" } },
    { id: `${id}-a`, role: "assistant", text: reply },
  ] as Block[];
}

it.each([128_000, 200_000, 1_000_000])(
  "rotates at 80% of a %i-token context window",
  (window) => {
    const session = (used: number) => ({
      providerSessionId: "native",
      context: { used, window },
      blocks: [],
    });
    expect(rotationReason(session(window * 0.8 - 1))).toBeUndefined();
    expect(rotationReason(session(window * 0.8))).toBe("context");
    expect(rotationReason(session(window))).toBe("context");
    expect(
      rotationReason({
        ...session(window),
        providerSessionId: undefined,
      }),
    ).toBeUndefined();
  },
);

it.each([45_000, 600_000])(
  "keeps a %i-token conversation in a large window across a long break",
  (used) => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(T0 + 48 * HOUR);
      const session = {
        providerSessionId: "native",
        context: { used, window: 1_000_000 },
        blocks: turn("hi", "hello", T0),
      };
      expect(rotationReason(session)).toBeUndefined();
    } finally {
      vi.useRealTimers();
    }
  },
);

it.each([
  undefined,
  { used: 900_000 },
  { used: 900_000, window: 0 },
  { used: 900_000, window: -1 },
  { used: 900_000, window: Number.NaN },
  { used: Number.NaN, window: 1_000_000 },
  { used: Number.POSITIVE_INFINITY, window: 1_000_000 },
  { used: -1, window: 1_000_000 },
])("does not rotate without a usable context percentage (%#)", (context) => {
  expect(
    rotationReason({ providerSessionId: "native", context, blocks: [] }),
  ).toBeUndefined();
});

it("briefs the fresh session with recent turns word for word and earlier ones as lines", () => {
  const blocks = [
    ...turn(
      "set up the release checklist",
      "Done, it is in docs/release.md",
      T0,
    ),
    ...turn("what is CI doing", "Two jobs are red on main", T0 + HOUR),
    ...turn("fix the lint job", "Fixed and pushed in #712", T0 + 2 * HOUR),
  ];
  const { rotation, brief } = planRotation(
    blocks,
    undefined,
    "context",
    T0 + 3 * HOUR,
  );
  const text = brief("mono-1");
  expect(text).toContain("<previous_conversation>");
  expect(text).toContain('sessions.read {"sessionId":"mono-1"}');
  expect(text).toMatch(
    /Earlier, oldest first:\n- .*User: set up the release checklist → You: Done, it is in docs\/release\.md/,
  );
  expect(text).toContain("Most recent, word for word:");
  expect(text).toContain("what is CI doing\n\nYou:\nTwo jobs are red on main");
  expect(text).toContain("fix the lint job\n\nYou:\nFixed and pushed in #712");
  expect(text).not.toContain("Read");
  expect(rotation.afterBlockId).toBe(blocks[blocks.length - 1].id);
  expect(rotation.earlier).toHaveLength(3);
});

it("remembers a completion report without overwriting the answer to the user's chat", () => {
  const blocks: Block[] = [
    ...turn("Explain the design", "The design uses a queue.", T0),
    {
      id: "notification",
      role: "user",
      text: "Hidden notification instructions",
      internal: true,
      monoSessionCompletion: {
        sessionId: "worker",
        title: "API fix",
        status: "completed",
      },
    },
    {
      id: "report",
      role: "assistant",
      text: "The API fix is done; all tests passed.",
    },
  ];
  const text = planRotation(blocks, undefined, "context", T0 + HOUR).brief(
    "mono",
  );
  expect(text).toContain(
    "Explain the design\n\nYou:\nThe design uses a queue.",
  );
  expect(text).toContain('reviewing session "API fix"');
  expect(text).toContain("The API fix is done; all tests passed.");
  expect(text).not.toContain("Hidden notification instructions");
});

it("carries earlier lines across rotations, briefing only what came since", () => {
  const first = [...turn("one", "1", T0), ...turn("two", "2", T0 + HOUR)];
  const planned = planRotation(first, undefined, "context", T0 + 2 * HOUR);
  // A saved rotation from before idle rotation was removed still carries over.
  saveMonoRotation("mono-1", { ...planned.rotation, reason: "idle" });
  const blocks = [
    ...first,
    ...turn("three", "3", T0 + 3 * HOUR),
    ...turn("four", "4", T0 + 4 * HOUR),
  ];
  const text = planRotation(
    blocks,
    loadMonoRotation("mono-1"),
    "context",
    T0 + 5 * HOUR,
  ).brief("mono-1");
  expect(text).toMatch(
    /Earlier, oldest first:\n- .*User: one → You: 1\n- .*User: two → You: 2/,
  );
  expect(text).toContain("three\n\nYou:\n3");
  expect(text).toContain("four\n\nYou:\n4");
});

it("marks where the fresh session's word-for-word part starts", () => {
  const blocks = [
    ...turn("set up the release checklist", "Done", T0),
    ...turn("what is CI doing", "Two jobs are red", T0 + HOUR),
    ...turn("fix the lint job", "Fixed in #712", T0 + 2 * HOUR),
  ];
  const { boundary } = planRotation(blocks, undefined, "idle", T0 + 3 * HOUR);
  expect(boundary).toMatchObject({
    kind: "rotation",
    trigger: "auto",
    reason: "idle",
    at: T0 + 3 * HOUR,
    kept: "recent",
    // The second-to-last exchange is the first one carried word for word.
    keptFromBlockId: blocks.find((block) => block.text === "what is CI doing")!
      .id,
  });
  expect(boundary.summary).toMatch(
    /^- .*User: set up the release checklist → You: Done$/,
  );
});

it("keeps nothing word for word when nothing happened since the last rotation", () => {
  const blocks = [...turn("one", "1", T0), ...turn("two", "2", T0 + HOUR)];
  const first = planRotation(blocks, undefined, "context", T0 + 2 * HOUR);
  const { boundary } = planRotation(
    blocks,
    first.rotation,
    "idle",
    T0 + 9 * HOUR,
  );
  expect(boundary.kept).toBe("none");
  expect(boundary.keptFromBlockId).toBeUndefined();
  expect(boundary.summary).toContain("User: two → You: 2");
});

it("reads past an earlier boundary in the chat", () => {
  const blocks: Block[] = [
    ...turn("one", "1", T0),
    {
      id: "compacted",
      role: "system",
      text: "Context compacted",
      contextBoundary: {
        kind: "compaction",
        trigger: "auto",
        at: T0,
        kept: "none",
      },
    },
    ...turn("two", "2", T0 + HOUR),
  ];
  const { boundary, brief } = planRotation(
    blocks,
    undefined,
    "context",
    T0 + 2 * HOUR,
  );
  expect(boundary.keptFromBlockId).toBe(blocks[0].id);
  expect(brief("m")).not.toContain("Context compacted");
});

it("keeps long messages bounded", () => {
  const blocks = turn("x".repeat(10_000), "y".repeat(10_000), T0);
  const text = planRotation(blocks, undefined, "context", T0 + HOUR).brief("m");
  expect(text.length).toBeLessThan(6_000);
  expect(text).toContain("[…]");
});

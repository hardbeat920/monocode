// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Block } from "../../sessions/model/session";
import {
  IDLE_ROTATE_MS,
  loadMonoBaseline,
  loadMonoRotation,
  planRotation,
  rotationReason,
  saveMonoBaseline,
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

it("rotates once the chat has grown well past a fresh session, or after a break", () => {
  const blocks = turn("hi", "hello", T0);
  const session = (used: number, window?: number) => ({
    providerSessionId: "native",
    context: { used, window },
    blocks,
  });
  expect(rotationReason(session(85_000), T0 + HOUR)).toBe("context");
  expect(rotationReason(session(60_000, 100_000), T0 + HOUR)).toBe("context");
  expect(rotationReason(session(30_000, 1_000_000), T0 + HOUR)).toBeUndefined();
  expect(rotationReason(session(45_000), T0 + IDLE_ROTATE_MS + HOUR)).toBe(
    "idle",
  );
  // Close to a fresh session's size, there is nothing to gain by dropping it.
  expect(
    rotationReason(session(30_000), T0 + IDLE_ROTATE_MS + HOUR),
  ).toBeUndefined();
  expect(
    rotationReason(session(45_000), T0 + IDLE_ROTATE_MS - HOUR),
  ).toBeUndefined();
  // Already fresh: the next turn starts a new provider session anyway.
  expect(
    rotationReason({ ...session(200_000), providerSessionId: undefined }, T0),
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
  saveMonoRotation("mono-1", planned.rotation);
  const blocks = [
    ...first,
    ...turn("three", "3", T0 + 3 * HOUR),
    ...turn("four", "4", T0 + 4 * HOUR),
  ];
  const text = planRotation(
    blocks,
    loadMonoRotation("mono-1"),
    "idle",
    T0 + 5 * HOUR,
  ).brief("mono-1");
  expect(text).toMatch(
    /Earlier, oldest first:\n- .*User: one → You: 1\n- .*User: two → You: 2/,
  );
  expect(text).toContain("three\n\nYou:\n3");
  expect(text).toContain("four\n\nYou:\n4");
});

it("keeps long messages bounded", () => {
  const blocks = turn("x".repeat(10_000), "y".repeat(10_000), T0);
  const text = planRotation(blocks, undefined, "context", T0 + HOUR).brief("m");
  expect(text.length).toBeLessThan(6_000);
  expect(text).toContain("[…]");
});

it("measures growth from the harness's own fresh-session size", () => {
  const blocks = turn("hi", "hello", T0);
  const session = {
    providerSessionId: "native",
    context: { used: 90_000 },
    blocks,
  };
  // A harness that starts at 40K has only grown 50K.
  expect(rotationReason(session, T0 + HOUR, 40_000)).toBeUndefined();
  expect(rotationReason(session, T0 + HOUR, 25_000)).toBe("context");
  saveMonoBaseline("mono-1", "claude", 38_412.6);
  expect(loadMonoBaseline("mono-1", "claude")).toBe(38_413);
  // Another harness starts from a different size; it measures its own.
  expect(loadMonoBaseline("mono-1", "codex")).toBeUndefined();
  saveMonoBaseline("mono-1", "claude", 0);
  expect(loadMonoBaseline("mono-1", "claude")).toBe(38_413);
});

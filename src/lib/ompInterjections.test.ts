import { afterEach, describe, expect, it, vi } from "vitest";
import type { OmpInterjectionAnchor } from "./fs";
import { backfillOmpInterjections } from "./ompInterjections";
import { newSession, type Block } from "./session";
import { getSession } from "./sessionStore";
import { foldableWork, foldedBlocks, groupTurnItems, groupTurns } from "../surfaces/transcriptActivity";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
afterEach(() => mocks.invoke.mockReset());

const anchor: OmpInterjectionAnchor = {
  id: "review",
  afterAssistantText: "The complete answer.",
  afterOccurrence: 1,
  text: "Check the fallback.",
  customType: "advisor",
  severity: "concern",
};

function oldBlocks(): Block[] {
  return [
    { id: "u", role: "user", text: "Go" },
    { id: "r1", role: "reasoning", text: "Thinking" },
    { id: "t1", role: "tool", text: "Check", tool: { kind: "shell", title: "Check", status: "completed" } },
    { id: "a1", role: "assistant", text: anchor.afterAssistantText },
    { id: "r2", role: "reasoning", text: "Rechecking" },
    { id: "t2", role: "tool", text: "Test", tool: { kind: "shell", title: "Test", status: "completed" } },
    { id: "a2", role: "assistant", text: "Checked." },
  ];
}

function foldedIds(blocks: Block[]) {
  const items = groupTurnItems(groupTurns(blocks)[0]);
  return foldedBlocks(items, foldableWork(items)!).map(block => block.id);
}

describe("OMP persisted interjection repair", () => {
  it("restores the complete answer outside the work fold without inventing a turn", () => {
    const before = oldBlocks();
    expect(foldedIds(before)).toContain("a1");
    const repaired = backfillOmpInterjections(before, [anchor]);
    expect(repaired[4]).toMatchObject({ id: "omp-interjection-review", role: "system", text: anchor.text, interjection: { customType: "advisor", severity: "concern" } });
    expect(groupTurns(repaired)).toHaveLength(1);
    expect(foldedIds(repaired)).toEqual(["r2", "t2"]);
    expect(before.map(block => block.id)).toEqual(["u", "r1", "t1", "a1", "r2", "t2", "a2"]);
  });

  it("targets the second identical answer rather than the first", () => {
    const blocks = oldBlocks();
    blocks[6] = { ...blocks[6], text: anchor.afterAssistantText };
    const repaired = backfillOmpInterjections(blocks, [{ ...anchor, afterOccurrence: 2 }]);
    expect(repaired.map(block => block.id)).toEqual(["u", "r1", "t1", "a1", "r2", "t2", "a2", "omp-interjection-review"]);
  });

  it("keeps unanchored progress prose folding and rejects approximate matches", () => {
    const blocks = oldBlocks();
    expect(backfillOmpInterjections(blocks, [])).toBe(blocks);
    expect(backfillOmpInterjections(blocks, [{ ...anchor, afterAssistantText: "The complete" }])).toBe(blocks);
    expect(backfillOmpInterjections(blocks, [{ ...anchor, afterAssistantText: ` ${anchor.afterAssistantText}` }])).toBe(blocks);
    expect(foldedIds(blocks)).toContain("a1");
  });

  it("does not duplicate repaired or already captured live boundaries", () => {
    const repaired = backfillOmpInterjections(oldBlocks(), [anchor]);
    expect(backfillOmpInterjections(repaired, [anchor])).toBe(repaired);
    const live = repaired.map(block => block.interjection ? { ...block, id: "random-live-id" } : block);
    expect(backfillOmpInterjections(live, [anchor])).toBe(live);
  });

  it("splits a coalesced answer only with an exact full continuation from the source", () => {
    const blocks: Block[] = [{ id: "a", role: "assistant", text: "First.\uD834\uDD1ESecond." }];
    const merged = { ...anchor, afterAssistantText: "First.\uD834\uDD1E", followingAssistantText: "Second." };
    const repaired = backfillOmpInterjections(blocks, [merged]);
    expect(repaired.map(block => [block.role, block.text])).toEqual([
      ["assistant", "First.\uD834\uDD1E"], ["system", anchor.text], ["assistant", "Second."],
    ]);
    expect(backfillOmpInterjections(repaired, [merged])).toBe(repaired);
    expect(backfillOmpInterjections(blocks, [{ ...merged, followingAssistantText: "Second" }])).toBe(blocks);
    expect(backfillOmpInterjections(blocks, [{ ...merged, followingAssistantText: undefined }])).toBe(blocks);
  });

  it("preserves source order and consumes live rows only once", () => {
    const first = backfillOmpInterjections(oldBlocks(), [anchor]);
    first[4] = { ...first[4], id: "live" };
    const second = { ...anchor, id: "review-again" };
    const repaired = backfillOmpInterjections(first, [anchor, second]);
    expect(repaired.filter(block => block.interjection).map(block => block.id)).toEqual([
      "live", "omp-interjection-review-again",
    ]);
    expect(backfillOmpInterjections(repaired, [anchor, second])).toBe(repaired);
  });

  it("preserves a split continuation after an existing live boundary and a new anchor", () => {
    const blocks: Block[] = [
      { id: "a", role: "assistant", text: "First.Second." },
      { id: "live", role: "system", text: anchor.text, interjection: { customType: "advisor", severity: "concern" } },
    ];
    const first = { ...anchor, afterAssistantText: "First.Second." };
    const second = { ...anchor, id: "second", afterAssistantText: "First.", followingAssistantText: "Second.", text: "Another review." };
    const repaired = backfillOmpInterjections(blocks, [first, second]);
    expect(repaired.map(block => [block.role, block.text])).toEqual([
      ["assistant", "First."], ["system", anchor.text],
      ["system", second.text], ["assistant", "Second."],
    ]);
    expect(backfillOmpInterjections(repaired, [first, second])).toBe(repaired);
    expect(backfillOmpInterjections(blocks, [first, second])).toEqual(repaired);
    expect(blocks[0].text).toBe("First.Second.");
  });

  it("matches subsequent anchors against a continuation created in the same pass", () => {
    const blocks: Block[] = [{ id: "a", role: "assistant", text: "First.Second." }];
    const first = { ...anchor, afterAssistantText: "First.", followingAssistantText: "Second." };
    const second = { ...anchor, id: "second", afterAssistantText: "Second.", text: "Second review." };
    const repaired = backfillOmpInterjections(blocks, [first, second]);
    expect(repaired.map(block => [block.role, block.text])).toEqual([
      ["assistant", "First."], ["system", first.text],
      ["assistant", "Second."], ["system", second.text],
    ]);
    expect(backfillOmpInterjections(repaired, [first, second])).toBe(repaired);
  });

  it("keeps all adjacent live notes before a recovered continuation", () => {
    const first = { ...anchor, afterAssistantText: "First.", followingAssistantText: "Second." };
    const second = { ...first, id: "second", text: "Second review." };
    const blocks: Block[] = [
      { id: "a", role: "assistant", text: "First.Second." },
      { id: "live-1", role: "system", text: first.text, interjection: { customType: "advisor", severity: "concern" } },
      { id: "live-2", role: "system", text: second.text, interjection: { customType: "advisor", severity: "concern" } },
    ];
    const repaired = backfillOmpInterjections(blocks, [first, second]);
    expect(repaired.map(block => [block.role, block.text])).toEqual([
      ["assistant", "First."], ["system", first.text],
      ["system", second.text], ["assistant", "Second."],
    ]);
    expect(backfillOmpInterjections(repaired, [first, second])).toBe(repaired);
  });

  it("adds tool-result and chained notes around an already repaired boundary without changing IDs", () => {
    const before = backfillOmpInterjections(oldBlocks(), [anchor]);
    const earlier = { ...anchor, id: "tool-note", text: "Tool review" };
    const later = { ...anchor, id: "chained-note", text: "Another review" };
    const repaired = backfillOmpInterjections(before, [earlier, anchor, later]);
    expect(repaired.filter(block => block.interjection).map(block => block.id)).toEqual([
      "omp-interjection-tool-note", "omp-interjection-review", "omp-interjection-chained-note",
    ]);
    expect(foldedIds(repaired)).toEqual(["r2", "t2"]);
    expect(repaired.filter(block => !block.interjection)).toEqual(oldBlocks());
    expect(backfillOmpInterjections(repaired, [earlier, anchor, later])).toBe(repaired);
  });

  it("matches both multipart representations with separate occurrences and exact split evidence", () => {
    const multipart = {
      ...anchor, afterAssistantText: "One\nTwo", afterAssistantTextConcat: "OneTwo",
      afterOccurrence: 1, afterConcatOccurrence: 2,
      followingAssistantText: "Three\nFour", followingAssistantTextConcat: "ThreeFour",
    };
    const blocks: Block[] = [
      { id: "earlier", role: "assistant", text: "OneTwo" },
      { id: "target", role: "assistant", text: "OneTwoThreeFour" },
    ];
    const repaired = backfillOmpInterjections(blocks, [multipart]);
    expect(repaired.map(block => block.text)).toEqual(["OneTwo", "OneTwo", anchor.text, "ThreeFour"]);
    expect(backfillOmpInterjections(repaired, [multipart])).toBe(repaired);
    const legacy: Block[] = [{ id: "target", role: "assistant", text: "One\nTwoThree\nFour" }];
    expect(backfillOmpInterjections(legacy, [multipart]).map(block => block.text)).toEqual([
      "One\nTwo", anchor.text, "Three\nFour",
    ]);
    expect(backfillOmpInterjections(blocks, [{ ...multipart, followingAssistantTextConcat: "Three" }])).toBe(blocks);
  });

  it("restores an entire note chain when only its last note has exact split evidence", () => {
    const first = { ...anchor, afterAssistantText: "First." };
    const last = { ...first, id: "last", text: "Last note", followingAssistantText: "Second." };
    const blocks: Block[] = [{ id: "a", role: "assistant", text: "First.Second." }];
    const repaired = backfillOmpInterjections(blocks, [first, last]);
    expect(repaired.map(block => block.text)).toEqual(["First.", first.text, last.text, "Second."]);
    expect(backfillOmpInterjections(repaired, [first, last])).toBe(repaired);
  });

  it("merges status-split prose with exact source evidence before placing later anchors", () => {
    const first = { ...anchor, afterAssistantText: "First.Second.", followingAssistantText: "Later." };
    const later = { ...anchor, id: "later", afterAssistantText: "Later." };
    const blocks: Block[] = [
      { id: "a", role: "assistant", text: "First." },
      { id: "status", role: "system", text: "Advisor reviewed this turn" },
      { id: "b", role: "assistant", text: "Second." },
      { id: "c", role: "assistant", text: "Later." },
    ];
    const repaired = backfillOmpInterjections(blocks, [first, later]);
    expect(repaired.map(block => [block.id, block.text])).toEqual([
      ["a", "First.Second."], ["omp-interjection-review", anchor.text],
      ["status", blocks[1].text], ["c", "Later."], ["omp-interjection-later", anchor.text],
    ]);
    expect(backfillOmpInterjections(repaired, [first, later])).toBe(repaired);
    expect(blocks[0].text).toBe("First.");
    expect(backfillOmpInterjections(blocks, [{ ...first, afterAssistantText: "First. Second." }])).toBe(blocks);
    const interleaved = blocks.map(block => block.id === "status"
      ? { ...block, interjection: { customType: "advisor" } } : block);
    expect(backfillOmpInterjections(interleaved, [first])).toBe(interleaved);
    const metadata = blocks.map(block => block.id === "b" ? { ...block, durationMs: 10 } : block);
    expect(backfillOmpInterjections(metadata, [first])).toBe(metadata);
  });

  it("counts merged messages in exact source occurrence order", () => {
    const blocks: Block[] = [
      { id: "a", role: "assistant", text: "The complete " },
      { id: "status", role: "system", text: "Reviewed" },
      { id: "b", role: "assistant", text: "answer." },
      { id: "c", role: "assistant", text: anchor.afterAssistantText },
    ];
    const repaired = backfillOmpInterjections(blocks, [{ ...anchor, afterOccurrence: 2 }]);
    expect(repaired.map(block => block.id)).toEqual(["a", "status", "c", "omp-interjection-review"]);
    expect(backfillOmpInterjections(repaired, [{ ...anchor, afterOccurrence: 2 }])).toBe(repaired);
  });
});

describe("persisted session loading", () => {
  function stored(harness: "omp" | "pi" = "omp", providerSessionId: string | undefined = "provider") {
    return { ...newSession(harness, "/tmp/project"), id: "repair-load", providerSessionId, blocks: oldBlocks() };
  }

  it("repairs before returning and persists only the first repair", async () => {
    let record = stored();
    let writes = 0;
    mocks.invoke.mockImplementation(async (command, args) => {
      if (command === "session_get") return record;
      if (command === "omp_session_interjections") return [anchor];
      if (command === "session_upsert") {
        writes += 1;
        record = { ...record, ...args.session };
        return record;
      }
      throw new Error(command);
    });
    const first = await getSession(record.id);
    expect(foldedIds(first!.blocks)).toEqual(["r2", "t2"]);
    const second = await getSession(record.id);
    expect(second!.blocks).toEqual(first!.blocks);
    expect(writes).toBe(1);
  });

  it("leaves other harnesses and unbound OMP sessions untouched", async () => {
    const other = stored("pi");
    mocks.invoke.mockResolvedValue(other);
    expect((await getSession(other.id))!.blocks).toEqual(other.blocks);
    const unbound = { ...stored(), providerSessionId: undefined };
    mocks.invoke.mockResolvedValue(unbound);
    expect((await getSession(unbound.id))!.blocks).toEqual(unbound.blocks);
    expect(mocks.invoke.mock.calls.map(call => call[0])).toEqual(["session_get", "session_get"]);
  });

  it("loads normally when the source command fails", async () => {
    const record = stored();
    mocks.invoke.mockImplementation(async command => {
      if (command === "session_get") return record;
      throw new Error("Log unavailable");
    });
    expect((await getSession(record.id))!.blocks).toEqual(record.blocks);
  });

  it("still displays recovered boundaries when persistence fails", async () => {
    const record = stored();
    mocks.invoke.mockImplementation(async command => {
      if (command === "session_get") return record;
      if (command === "omp_session_interjections") return [anchor];
      throw new Error("Database unavailable");
    });
    expect(foldedIds((await getSession(record.id))!.blocks)).toEqual(["r2", "t2"]);
  });
});

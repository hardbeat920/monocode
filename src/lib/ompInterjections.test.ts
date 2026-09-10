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

import { afterEach, describe, expect, it, vi } from "vitest";
import type { OmpAssistantText, OmpInterjectionAnchor } from "./fs";
import { backfillOmpInterjections, ompStatusSplitTexts } from "./ompInterjections";
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

  /** Ordered active-path source messages; a string is one message in both forms. */
  function source(...messages: (string | OmpAssistantText)[]): OmpAssistantText[] {
    return messages.map(message => typeof message === "string" ? { text: message, concat: message } : message);
  }

  it("merges status-split prose with the next source message before placing later anchors", () => {
    const first = { ...anchor, afterAssistantText: "First.Second.", followingAssistantText: "Later." };
    const later = { ...anchor, id: "later", afterAssistantText: "Later." };
    const blocks: Block[] = [
      { id: "a", role: "assistant", text: "First." },
      { id: "status", role: "system", text: "Advisor reviewed this turn" },
      { id: "b", role: "assistant", text: "Second." },
      { id: "c", role: "assistant", text: "Later." },
    ];
    const messages = source("First.Second.", "Later.");
    const repaired = backfillOmpInterjections(blocks, [first, later], messages);
    expect(repaired.map(block => [block.id, block.text])).toEqual([
      ["a", "First.Second."], ["omp-interjection-review", anchor.text],
      ["status", blocks[1].text], ["c", "Later."], ["omp-interjection-later", anchor.text],
    ]);
    expect(backfillOmpInterjections(repaired, [first, later], messages)).toBe(repaired);
    expect(blocks[0].text).toBe("First.");
    expect(backfillOmpInterjections(blocks, [{ ...first, afterAssistantText: "First. Second." }], source("First. Second.", "Later."))).toBe(blocks);
    const interleaved = blocks.map(block => block.id === "status"
      ? { ...block, interjection: { customType: "advisor" } } : block);
    expect(backfillOmpInterjections(interleaved, [first], messages)).toBe(interleaved);
    const metadata = blocks.map(block => block.id === "b" ? { ...block, durationMs: 10 } : block);
    expect(backfillOmpInterjections(metadata, [first], messages)).toBe(metadata);
  });

  it("never merges on anchor occurrences alone", () => {
    const blocks: Block[] = [
      { id: "a", role: "assistant", text: "First." },
      { id: "status", role: "system", text: "Reviewed" },
      { id: "b", role: "assistant", text: "Second." },
    ];
    const anchors = [{ ...anchor, afterAssistantText: "First.Second.", afterAssistantTextConcat: "First.Second.", afterConcatOccurrence: 1 }];
    expect(backfillOmpInterjections(blocks, anchors)).toBe(blocks);
    expect(backfillOmpInterjections(blocks, [{ ...anchors[0], afterAssistantText: "First.\nSecond.", afterOccurrence: 2 }])).toBe(blocks);
    const repaired = backfillOmpInterjections(blocks, anchors, source("First.Second."));
    expect(repaired.map(block => block.id)).toEqual(["a", "omp-interjection-review", "status"]);
  });

  it("keeps a split whose fragments are the next two source messages before the real complete answer", () => {
    const blocks: Block[] = [
      { id: "a", role: "assistant", text: "First." },
      { id: "status", role: "system", text: "Reviewed" },
      { id: "b", role: "assistant", text: "Second." },
      { id: "c", role: "assistant", text: "First.Second." },
    ];
    const messages = source("First.", "Second.", "First.Second.");
    expect(ompStatusSplitTexts(blocks)).toEqual(["First.Second."]);
    expect(backfillOmpInterjections(blocks, [], messages)).toBe(blocks);
    const anchors = [{ ...anchor, afterAssistantText: "First.Second." }];
    const repaired = backfillOmpInterjections(blocks, anchors, messages);
    expect(repaired.map(block => block.id)).toEqual(["a", "status", "b", "c", "omp-interjection-review"]);
    expect(backfillOmpInterjections(repaired, anchors, messages)).toBe(repaired);
  });

  it("leaves a split whose first fragment is its own source message and anchors the later complete answer", () => {
    const blocks: Block[] = [
      { id: "a", role: "assistant", text: "The complete " },
      { id: "status", role: "system", text: "Reviewed" },
      { id: "b", role: "assistant", text: "answer." },
      { id: "c", role: "assistant", text: anchor.afterAssistantText },
    ];
    const messages = source("The complete ", "answer.", anchor.afterAssistantText);
    const repaired = backfillOmpInterjections(blocks, [anchor], messages);
    expect(repaired.map(block => block.id)).toEqual(["a", "status", "b", "c", "omp-interjection-review"]);
    expect(backfillOmpInterjections(repaired, [anchor], messages)).toBe(repaired);
    expect(backfillOmpInterjections(blocks, [{ ...anchor, afterOccurrence: 2 }], messages)).toBe(blocks);
  });

  it("keeps streaming fragments and metadata even with a matching source message", () => {
    const blocks: Block[] = [
      { id: "a", role: "assistant", text: "First." },
      { id: "status", role: "system", text: "Reviewed" },
      { id: "b", role: "assistant", text: "Second." },
    ];
    for (const index of [0, 2]) {
      const streaming = blocks.map((block, i) => i === index ? { ...block, streaming: true } : block);
      expect(ompStatusSplitTexts(streaming)).toEqual([]);
      expect(backfillOmpInterjections(streaming, [], source("First.Second."))).toBe(streaming);
    }
    const metadata = blocks.map(block => block.id === "b" ? { ...block, durationMs: 10 } : block);
    expect(ompStatusSplitTexts(metadata)).toEqual([]);
    expect(backfillOmpInterjections(metadata, [], source("First.Second."))).toBe(metadata);
  });

  function split(id: string): Block[] {
    return [
      { id, role: "assistant", text: "First." },
      { id: `${id}-status`, role: "system", text: "Reviewed" },
      { id: `${id}-end`, role: "assistant", text: "Second." },
    ];
  }

  it.each([
    ["newline", { text: "First.\nSecond.", concat: "First.Second." }],
    ["concat", { text: "First.Second.", concat: "First.Second." }],
  ])("consumes one source message for one split, form: %s", (_form, message) => {
    const blocks = [...split("a"), ...split("b")];
    const messages = source(message);
    const repaired = backfillOmpInterjections(blocks, [], messages);
    expect(repaired.find(block => block.id === "a")!.text).toBe("First.Second.");
    expect(repaired.filter(block => block.id.startsWith("b"))).toEqual(split("b"));
    expect(backfillOmpInterjections(repaired, [], messages)).toBe(repaired);
  });

  it("merges a true split followed by a different complete message", () => {
    const blocks: Block[] = [...split("a"), { id: "c", role: "assistant", text: "Later." }];
    const repaired = backfillOmpInterjections(blocks, [], source("First.Second.", "Later."));
    expect(repaired.map(block => [block.id, block.text])).toEqual([
      ["a", "First.Second."], ["a-status", "Reviewed"], ["c", "Later."],
    ]);
  });

  it.each([false, true])("lets an unsplit answer occupy its source slot before a later candidate, anchored: %s", anchored => {
    const blocks: Block[] = [{ id: "complete", role: "assistant", text: "First.Second." }, ...split("a")];
    const anchors = anchored ? [{ ...anchor, afterAssistantText: "First.Second." }] : [];
    const repaired = backfillOmpInterjections(blocks, anchors, source("First.Second."));
    expect(repaired.filter(block => block.id.startsWith("a"))).toEqual(split("a"));
    const both = backfillOmpInterjections(blocks, anchors, source("First.Second.", "First.Second."));
    expect(both.filter(block => block.role === "assistant").map(block => [block.id, block.text])).toEqual([
      ["complete", "First.Second."], ["a", "First.Second."],
    ]);
  });

  it("repairs two splits only with two source messages", () => {
    const blocks = [...split("a"), ...split("b")];
    const messages = source("First.Second.", "First.Second.");
    const repaired = backfillOmpInterjections(blocks, [], messages);
    expect(repaired.map(block => block.id)).toEqual(["a", "a-status", "b", "b-status"]);
    expect(repaired.filter(block => block.role === "assistant").map(block => block.text)).toEqual(["First.Second.", "First.Second."]);
    expect(backfillOmpInterjections(repaired, [], messages)).toBe(repaired);
  });

  it("keeps anchor evidence from adding to the source sequence", () => {
    const blocks = [...split("a"), ...split("b")];
    const anchors = [{ ...anchor, afterAssistantText: "First.Second." }];
    const messages = source("First.Second.");
    const repaired = backfillOmpInterjections(blocks, anchors, messages);
    expect(repaired.filter(block => block.id.startsWith("b"))).toEqual(split("b"));
    expect(backfillOmpInterjections(repaired, anchors, messages)).toBe(repaired);
  });

  it("does not bind a later anchor occurrence or unnumbered following text to an earlier split", () => {
    const blocks = split("a");
    expect(backfillOmpInterjections(blocks, [{ ...anchor, afterAssistantText: "First.Second.", afterOccurrence: 2 }])).toBe(blocks);
    expect(backfillOmpInterjections(blocks, [{ ...anchor, followingAssistantText: "First.Second." }])).toBe(blocks);
  });

  it("merges ten identical splits only against the three source messages in order", () => {
    const blocks = Array.from({ length: 10 }, (_, i) => split(String(i))).flat();
    const messages = source("First.Second.", "First.Second.", "First.Second.");
    const repaired = backfillOmpInterjections(blocks, [], messages);
    expect(repaired.filter(block => block.role === "assistant" && block.text === "First.Second.").map(block => block.id)).toEqual(["0", "1", "2"]);
    expect(repaired.filter(block => block.id.endsWith("-end"))).toHaveLength(7);
    expect(backfillOmpInterjections(repaired, [], messages)).toBe(repaired);
  });

  it("realigns a complete block past source messages the transcript never stored", () => {
    const blocks: Block[] = [{ id: "complete", role: "assistant", text: "Intro." }, ...split("a")];
    const repaired = backfillOmpInterjections(blocks, [], source("Setup.", "Intro.", "First.Second."));
    expect(repaired.map(block => block.id)).toEqual(["complete", "a", "a-status"]);
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

  it.each([true, false])("repairs unanchored status splits only when verified: %s", async verified => {
    let record = { ...stored(), blocks: [
      { id: "a", role: "assistant", text: "First." },
      { id: "status", role: "system", text: "Reviewed" },
      { id: "b", role: "assistant", text: "Second." },
      { id: "u", role: "user", text: "Continue" },
    ] as Block[] };
    const original = record.blocks;
    let writes = 0;
    mocks.invoke.mockImplementation(async (command, args) => {
      if (command === "session_get") return record;
      if (command === "omp_session_interjections") return [];
      if (command === "omp_active_assistant_texts") {
        expect(args).toEqual({ providerSessionId: "provider" });
        return verified ? [{ text: "First.Second.", concat: "First.Second." }] : [];
      }
      if (command === "session_upsert") {
        writes++;
        record = { ...record, ...args.session };
        return record;
      }
      throw new Error(command);
    });
    const first = await getSession(record.id);
    expect(first!.blocks).toEqual(verified
      ? [{ ...original[0], text: "First.Second." }, original[1], original[3]] : original);
    expect((await getSession(record.id))!.blocks).toEqual(first!.blocks);
    expect(writes).toBe(verified ? 1 : 0);
  });

  it.each([false, true])("loads duplicate text without reusing occupied source evidence, complete first: %s", async complete => {
    const first: Block[] = complete ? [{ id: "a", role: "assistant", text: "First.Second." }] : [
      { id: "a", role: "assistant", text: "First." },
      { id: "s1", role: "system", text: "Reviewed" },
      { id: "b", role: "assistant", text: "Second." },
    ];
    const later: Block[] = [
      { id: "c", role: "assistant", text: "First." },
      { id: "s2", role: "system", text: "Reviewed" },
      { id: "d", role: "assistant", text: "Second." },
    ];
    const record = { ...stored(), blocks: [...first, ...later] };
    mocks.invoke.mockImplementation(async (command, args) => {
      if (command === "session_get") return record;
      if (command === "omp_session_interjections") return [];
      if (command === "omp_active_assistant_texts") return [{ text: "First.Second.", concat: "First.Second." }];
      if (command === "session_upsert") return args.session;
      throw new Error(command);
    });
    const loaded = (await getSession(record.id))!;
    expect(loaded.blocks[0]).toMatchObject({ id: "a", text: "First.Second." });
    expect(loaded.blocks.slice(-3)).toEqual(later);
    expect(loaded.blocks).toHaveLength(complete ? 4 : 5);
  });

  it("loads a split of two source messages before their concatenation unchanged", async () => {
    const record = { ...stored(), blocks: [
      { id: "a", role: "assistant", text: "First." },
      { id: "status", role: "system", text: "Reviewed" },
      { id: "b", role: "assistant", text: "Second." },
      { id: "c", role: "assistant", text: "First.Second." },
    ] as Block[] };
    const commands: string[] = [];
    mocks.invoke.mockImplementation(async command => {
      commands.push(command);
      if (command === "session_get") return record;
      if (command === "omp_session_interjections") return [];
      if (command === "omp_active_assistant_texts") {
        return ["First.", "Second.", "First.Second."].map(text => ({ text, concat: text }));
      }
      throw new Error(command);
    });
    expect((await getSession(record.id))!.blocks).toEqual(record.blocks);
    expect(commands).toEqual(["session_get", "omp_session_interjections", "omp_active_assistant_texts"]);
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

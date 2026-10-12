import { describe, expect, it } from "vitest";
import type { Block } from "./session";
import { groupTurns } from "./transcriptActivity";
import { TranscriptTurnCache } from "./transcriptTurnCache";

function conversation(): Block[] {
  return Array.from({ length: 20 }, (_, index): Block[] => [
    { id: `u${index}`, role: "user", text: `Question ${index}` },
    { id: `a${index}`, role: "assistant", text: `Answer ${index}` },
  ]).flat();
}

describe("transcript turn cache", () => {
  it("reuses settled history as the latest reply streams", () => {
    const cache = new TranscriptTurnCache();
    const blocks = conversation();
    const before = cache.group(blocks);
    const previousItems = before.map((turn, index) =>
      cache.turnItems(turn, index < before.length - 1),
    );
    const updated = blocks.slice();
    updated[updated.length - 1] = {
      ...updated[updated.length - 1],
      text: "More output",
    };
    const after = cache.group(updated);
    for (let index = 0; index < 19; index++) {
      expect(after[index]).toBe(before[index]);
      expect(cache.turnItems(after[index], true)).toBe(previousItems[index]);
    }
    expect(after[19]).not.toBe(before[19]);
    expect(cache.turnItems(after[19], false)).not.toBe(previousItems[19]);
  });

  it("updates an edited historical turn without changing its neighbors", () => {
    const cache = new TranscriptTurnCache();
    const blocks = conversation();
    const before = cache.group(blocks);
    const edited = blocks.slice();
    edited[3] = { ...edited[3], text: "Corrected answer" };
    const after = cache.group(edited);
    expect(after[1]).not.toBe(before[1]);
    expect(cache.turnItems(after[1], true)).toContainEqual({
      type: "block",
      block: edited[3],
    });
    expect(after[0]).toBe(before[0]);
    expect(after[2]).toBe(before[2]);
  });

  it("keeps live and settled folding distinct", () => {
    const cache = new TranscriptTurnCache();
    const turn: Block[] = [
      { id: "u", role: "user", text: "Check this" },
      {
        id: "interjection",
        role: "system",
        text: "Review note",
        interjection: { source: "omp-advisor", severity: "concern" },
      },
    ];
    const live = cache.turnItems(turn, false);
    const settled = cache.turnItems(turn, true);
    expect(live[1].type).toBe("block");
    expect(settled[1].type).toBe("activity");
    expect(cache.turnItems(turn, false)).toBe(live);
    expect(cache.turnItems(turn, true)).toBe(settled);
  });

  it("preserves handoffs, managed turns and rewinds", () => {
    const cache = new TranscriptTurnCache();
    const blocks: Block[] = [
      ...conversation().slice(0, 4),
      { id: "handoff", role: "handoff", text: "Switch provider" },
      { id: "internal", role: "user", text: "Continue", internal: true },
      { id: "reply", role: "assistant", text: "Done" },
    ];
    expect(cache.group(blocks)).toEqual(groupTurns(blocks));
    expect(cache.group(blocks, true)).toEqual(groupTurns(blocks, true));
    expect(cache.group(blocks)).toEqual(groupTurns(blocks));
    expect(cache.group(blocks.slice(0, 2))).toEqual(
      groupTurns(blocks.slice(0, 2)),
    );
  });

  it("reuses groups for local renders and equivalent immutable arrays", () => {
    const cache = new TranscriptTurnCache();
    const blocks = conversation();
    const before = cache.group(blocks);
    expect(cache.group(blocks)).toBe(before);
    expect(cache.group(blocks.slice())).toBe(before);
  });

  it("caches Mono follow-ups together and updates when inline work changes", () => {
    const cache = new TranscriptTurnCache();
    const blocks: Block[] = [
      { id: "u", role: "user", text: "Inspect this" },
      { id: "a", role: "assistant", text: "Checking" },
      { id: "follow-up", role: "user", text: "And this", sentAt: 10 },
      { id: "reply", role: "assistant", text: "Done" },
    ];
    expect(cache.group(blocks)).toHaveLength(2);
    const monoTurns = cache.group(blocks, false, true);
    expect(monoTurns).toHaveLength(1);
    expect(monoTurns[0]).toEqual(blocks);
    const items = cache.turnItems(monoTurns[0], true, { inlineWork: true });
    expect(cache.group(blocks.slice(), false, true)).toBe(monoTurns);
    expect(cache.turnItems(monoTurns[0], true, { inlineWork: true })).toBe(
      items,
    );
    expect(cache.group(blocks)).toHaveLength(2);
  });

  it("keeps cached Mono narration distinct from ordinary and settled work", () => {
    const cache = new TranscriptTurnCache();
    const turn: Block[] = [
      { id: "u", role: "user", text: "Inspect this" },
      { id: "opening", role: "assistant", text: "Checking" },
      {
        id: "tool",
        role: "tool",
        text: "Inspect files",
        tool: { kind: "shell", status: "completed" },
      },
      { id: "reply", role: "assistant", text: "Done" },
    ];
    const live = cache.turnItems(turn, false, { inlineWork: true });
    expect(live[1]).toEqual({ type: "activity", blocks: turn.slice(1) });
    const settled = cache.turnItems(turn, true, { inlineWork: true });
    expect(settled[2]).toEqual({ type: "block", block: turn[3] });
    const ordinary = cache.turnItems(turn, false);
    expect(ordinary[3]).toEqual({ type: "block", block: turn[3] });
    expect(cache.turnItems(turn, false, { inlineWork: true })).toBe(live);
    expect(cache.turnItems(turn, true, { inlineWork: true })).toBe(settled);
  });

  it("refreshes reply visibility when a queued follow-up is delivered without changing blocks", () => {
    const cache = new TranscriptTurnCache();
    const turn: Block[] = [
      { id: "user", role: "user", text: "Review" },
      { id: "intro", role: "assistant", text: "Checking." },
      { id: "follow-up", role: "user", text: "Status?", sentAt: 10 },
      { id: "reply", role: "assistant", text: "Checking browser security." },
    ];
    const pending = cache.turnItems(turn, false, {
      inlineWork: true,
      undeliveredMessageIds: new Set(["follow-up"]),
    });
    expect(pending).not.toContainEqual({ type: "block", block: turn[3] });
    const delivered = cache.turnItems(turn, false, { inlineWork: true });
    expect(delivered).toContainEqual({ type: "block", block: turn[3] });
    expect(cache.turnItems(turn, false, { inlineWork: true })).toBe(delivered);
    expect(delivered).not.toBe(pending);
  });

  it("retains hidden completion prompts for identity while respecting managed visibility", () => {
    const cache = new TranscriptTurnCache();
    const blocks: Block[] = [
      { id: "u", role: "user", text: "Inspect this", startedAt: 0 },
      { id: "a", role: "assistant", text: "Checking" },
      {
        id: "completion",
        role: "user",
        text: "Private completion prompt",
        internal: true,
        appRequestId: "mono-completion-result",
        // A day later, so the reply opens a turn of its own.
        startedAt: 24 * 60 * 60 * 1000,
      },
      { id: "reply", role: "assistant", text: "Done" },
    ];
    const turns = cache.group(blocks, false, true);
    expect(turns[1][0]).toBe(blocks[2]);
    const visible = cache.turnItems(turns[1], true, { inlineWork: true });
    expect(visible).toEqual([{ type: "block", block: blocks[3] }]);
    const managed = cache.turnItems(turns[1], true, {
      managed: true,
      inlineWork: true,
    });
    expect(managed[0]).toEqual({ type: "block", block: blocks[2] });
    expect(cache.turnItems(turns[1], true, { inlineWork: true })).toBe(visible);
  });
  it("keeps each merged Mono run's answer when a later run did more work", () => {
    const cache = new TranscriptTurnCache();
    const shell = (id: string): Block => ({
      id,
      role: "tool",
      text: "bash ls",
      tool: { kind: "shell", title: "bash ls", status: "completed" },
    });
    const blocks: Block[] = [
      { id: "thanks", role: "user", text: "thanks :)", startedAt: 0 },
      { id: "welcome", role: "assistant", text: "You're welcome!" },
      {
        id: "completion",
        role: "user",
        text: "Private completion prompt",
        internal: true,
        appRequestId: "mono-completion-review",
        startedAt: 60_000,
      },
      shell("check"),
      { id: "cancelled", role: "assistant", text: "The review was cancelled." },
    ];
    const turns = cache.group(blocks, false, true);
    expect(turns).toHaveLength(1);
    for (const settled of [true, false]) {
      const prose = cache
        .turnItems(turns[0], settled, { inlineWork: true })
        .flatMap((item) =>
          item.type === "block" && item.block.role === "assistant"
            ? [item.block.id]
            : [],
        );
      expect(prose).toContain("welcome");
      if (settled) expect(prose).toEqual(["welcome", "cancelled"]);
    }
  });
});

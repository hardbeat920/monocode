import { describe, expect, it } from "vitest";
import type { Block, ContextKept } from "./session";
import { outOfContextIds, resendableIds } from "./contextBoundary";

function boundary(id: string, kept: ContextKept): Block {
  return {
    id,
    role: "system",
    text: "Context compacted",
    contextBoundary: { kind: "compaction", trigger: "auto", at: 1, kept },
  };
}

const user = (id: string): Block => ({ id, role: "user", text: id });
const reply = (id: string): Block => ({ id, role: "assistant", text: id });
const tool = (id: string): Block => ({ id, role: "tool", text: id });

describe("outOfContextIds", () => {
  it("is empty before any compaction", () => {
    expect(outOfContextIds([user("u1"), reply("a1")])).toEqual(new Set());
  });

  it("covers everything before a boundary that kept only a summary", () => {
    expect(
      outOfContextIds([
        user("u1"),
        tool("t1"),
        reply("a1"),
        boundary("b1", "none"),
        user("u2"),
      ]),
    ).toEqual(new Set(["u1", "t1", "a1"]));
  });

  it("spares your prompts when the harness kept them", () => {
    expect(
      outOfContextIds([
        user("u1"),
        tool("t1"),
        reply("a1"),
        boundary("b1", "user-messages"),
        reply("a2"),
      ]),
    ).toEqual(new Set(["t1", "a1"]));
  });

  it("claims nothing when it cannot tell what was kept", () => {
    for (const kept of ["recent", "unknown"] as const) {
      expect(
        outOfContextIds([user("u1"), reply("a1"), boundary("b1", kept)]),
      ).toEqual(new Set());
    }
  });

  it("goes by the latest boundary only", () => {
    expect(
      outOfContextIds([
        user("u1"),
        boundary("b1", "none"),
        user("u2"),
        reply("a2"),
        boundary("b2", "recent"),
        user("u3"),
      ]),
    ).toEqual(new Set());
    expect(
      outOfContextIds([
        user("u1"),
        boundary("b1", "recent"),
        reply("a2"),
        boundary("b2", "none"),
      ]),
    ).toEqual(new Set(["u1", "b1", "a2"]));
  });
});

describe("outOfContextIds with a known kept point", () => {
  const rotation = (keptFromBlockId?: string): Block => ({
    id: "r1",
    role: "system",
    text: "Fresh session started",
    contextBoundary: {
      kind: "rotation",
      trigger: "auto",
      at: 1,
      kept: "recent",
      ...(keptFromBlockId ? { keptFromBlockId } : {}),
    },
  });

  it("covers exactly what comes before the first kept block", () => {
    expect(
      outOfContextIds([
        user("u1"),
        reply("a1"),
        user("u2"),
        reply("a2"),
        rotation("u2"),
        user("u3"),
      ]),
    ).toEqual(new Set(["u1", "a1"]));
  });

  it("claims nothing when the kept block is not in view", () => {
    expect(
      outOfContextIds([
        user("u2"),
        reply("a2"),
        rotation("paged-out"),
        user("u3"),
      ]),
    ).toEqual(new Set());
  });
});

describe("resendableIds", () => {
  const compaction = (kept: ContextKept): Block => boundary("b1", kept);
  const above = [user("u1"), tool("t1"), reply("a1")];

  it("offers nothing before any boundary", () => {
    expect(resendableIds(above)).toEqual(new Set());
  });

  it("offers messages, never tool rows, from before a summary-only boundary", () => {
    expect(resendableIds([...above, compaction("none"), user("u2")])).toEqual(
      new Set(["u1", "a1"]),
    );
  });

  it("offers only replies where the harness kept your prompts", () => {
    expect(
      resendableIds([...above, compaction("user-messages"), user("u2")]),
    ).toEqual(new Set(["a1"]));
  });

  it("offers everything above when what was kept is not known", () => {
    for (const kept of ["recent", "unknown"] as const) {
      expect(resendableIds([...above, compaction(kept)])).toEqual(
        new Set(["u1", "a1"]),
      );
    }
  });

  it("stops at a known kept point", () => {
    const rotation: Block = {
      ...boundary("r1", "recent"),
      contextBoundary: {
        kind: "rotation",
        trigger: "auto",
        at: 1,
        kept: "recent",
        keptFromBlockId: "u2",
      },
    };
    expect(
      resendableIds([...above, user("u2"), reply("a2"), rotation]),
    ).toEqual(new Set(["u1", "a1"]));
  });

  it("skips app-written turns and drafts", () => {
    expect(
      resendableIds([
        { id: "i1", role: "user", text: "internal", internal: true },
        { id: "d1", role: "user", text: "draft", draft: true },
        compaction("none"),
      ]),
    ).toEqual(new Set());
  });
});

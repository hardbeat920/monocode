import { describe, expect, it } from "vitest";
import {
  acknowledgeComposerInsert,
  appendComposerInsert,
  composerSeedForAddToChat,
  consumeComposerInsert,
  isMarkdownBlockquotePosition,
  type ComposerInsertRequest,
} from "./quoteDraft";
import { splitChatContext, type ChatContextItem } from "./chatContext";

const code: ChatContextItem = {
  kind: "code",
  path: "src/value.ts",
  startLine: 3,
  endLine: 5,
};

describe("appendComposerInsert", () => {
  it.each([
    ["", "Comment\n\n"],
    ["draft", "draft\n\nComment\n\n"],
    ["draft\n", "draft\n\nComment\n\n"],
    ["draft\n\n", "draft\n\nComment\n\n"],
  ])("separates an existing draft %#", (draft, expected) => {
    expect(appendComposerInsert(draft, " Comment\r\n")).toBe(expected);
  });

  it("ignores whitespace-only text", () => {
    expect(appendComposerInsert("draft", "  \n ")).toBe("draft");
  });
});

describe("composerSeedForAddToChat", () => {
  it("seeds a new composer with the chip and no typed text", () => {
    expect(splitChatContext(composerSeedForAddToChat(code))).toEqual({
      text: "",
      items: [code],
    });
  });
});

describe("isMarkdownBlockquotePosition", () => {
  it("recognizes tokens after nested quote markers", () => {
    const text = "plain /one\n> /two\n  >> @file";
    expect(isMarkdownBlockquotePosition(text, text.indexOf("/one"))).toBe(
      false,
    );
    expect(isMarkdownBlockquotePosition(text, text.indexOf("/two"))).toBe(true);
    expect(isMarkdownBlockquotePosition(text, text.indexOf("@file"))).toBe(
      true,
    );
  });
});

describe("consumeComposerInsert", () => {
  const request: ComposerInsertRequest = {
    id: 1,
    kind: "context",
    item: code,
  };

  it("adds a context chip once and leaves the draft alone", () => {
    expect(consumeComposerInsert("draft", [], null, request)).toEqual({
      draft: "draft",
      context: [code],
      consumedId: 1,
      changed: true,
    });
    expect(consumeComposerInsert("draft", [], 1, request)).toEqual({
      draft: "draft",
      context: [],
      consumedId: 1,
      changed: false,
    });
  });

  it("does not attach the same chip twice", () => {
    expect(
      consumeComposerInsert("", [code], 1, { ...request, id: 2 }),
    ).toMatchObject({ context: [code], consumedId: 2, changed: false });
  });

  it("inserts text into the draft", () => {
    expect(
      consumeComposerInsert("draft", [code], null, {
        id: 3,
        kind: "text",
        text: "Note: Auth\n\nUse a cookie.",
      }),
    ).toEqual({
      draft: "draft\n\nNote: Auth\n\nUse a cookie.\n\n",
      context: [code],
      consumedId: 3,
      changed: true,
    });
  });
});

describe("acknowledgeComposerInsert", () => {
  it("clears only the request that was acknowledged", () => {
    const current: ComposerInsertRequest = { id: 2, kind: "text", text: "x" };
    expect(acknowledgeComposerInsert(current, 2)).toBeUndefined();
    expect(acknowledgeComposerInsert(current, 1)).toBe(current);
  });
});

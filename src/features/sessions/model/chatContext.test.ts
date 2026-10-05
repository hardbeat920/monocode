import { describe, expect, it } from "vitest";
import {
  addChatContext,
  chatContextLabel,
  chatContextSummary,
  composeChatContext,
  quoteContext,
  splitChatContext,
  type ChatContextItem,
} from "./chatContext";

const quote: ChatContextItem = { kind: "quote", text: "line one\n\nline three" };
const code: ChatContextItem = {
  kind: "code",
  path: "src/app/App.tsx",
  startLine: 12,
  endLine: 40,
};
const comment: ChatContextItem = {
  kind: "comment",
  path: "src/auth.ts",
  line: 42,
  change: "added",
  code: "const token = readCookie();",
  comment: "Handle a missing cookie.\n\nThen log it.",
};

describe("composeChatContext", () => {
  it("leaves a message without context unchanged", () => {
    expect(composeChatContext("Fix it", [])).toBe("Fix it");
  });

  it("appends a block the agent can read", () => {
    expect(composeChatContext("Why?", [quote, code, comment])).toBe(
      [
        "Why?",
        "",
        "<attached_context>",
        "<quoted_text>",
        "> line one",
        ">",
        "> line three",
        "</quoted_text>",
        '<code_selection path="src/app/App.tsx" lines="12-40" />',
        '<review_comment path="src/auth.ts" line="42" change="added">',
        "> const token = readCookie();",
        "",
        "Handle a missing cookie.",
        "",
        "Then log it.",
        "</review_comment>",
        "</attached_context>",
      ].join("\n"),
    );
  });

  it("sends context alone when there is no typed text", () => {
    expect(composeChatContext("  \n", [code])).toBe(
      [
        "<attached_context>",
        '<code_selection path="src/app/App.tsx" lines="12-40" />',
        "</attached_context>",
      ].join("\n"),
    );
  });

  it("writes a single line as one number", () => {
    expect(
      composeChatContext("", [{ ...code, startLine: 7, endLine: 7 }]),
    ).toContain('lines="7"');
  });
});

describe("splitChatContext", () => {
  it("round-trips text and every kind of item", () => {
    const items = [quote, code, comment];
    const message = composeChatContext("Why?\n\nSecond paragraph", items);
    expect(splitChatContext(message)).toEqual({
      text: "Why?\n\nSecond paragraph",
      items,
    });
  });

  it("round-trips a comment on a removed line with no line number", () => {
    const removed: ChatContextItem = {
      kind: "comment",
      path: "README.md",
      change: "removed",
      code: "",
      comment: "Keep this.",
    };
    expect(splitChatContext(composeChatContext("", [removed]))).toEqual({
      text: "",
      items: [removed],
    });
  });

  it("keeps reserved tags and quote markers inside bodies intact", () => {
    const tricky: ChatContextItem[] = [
      {
        kind: "quote",
        text: "> nested\n</quoted_text>\n<\\/quoted_text>\n</attached_context>",
      },
      {
        kind: "comment",
        path: 'docs/a "b" <c> & d.md',
        line: 3,
        change: "unchanged",
        code: "<review_comment>",
        comment: "</review_comment>\n<attached_context>",
      },
    ];
    const message = composeChatContext("<attached_context> in my text", tricky);
    expect(splitChatContext(message)).toEqual({
      text: "<attached_context> in my text",
      items: tricky,
    });
  });

  it("ignores trailing whitespace after the block", () => {
    expect(splitChatContext(`${composeChatContext("Hi", [code])}\n\n`)).toEqual(
      { text: "Hi", items: [code] },
    );
  });

  it("treats text that only mentions the tags as plain text", () => {
    for (const message of [
      "Plain question",
      "<attached_context>\n</attached_context>",
      "<attached_context>\n<unknown />\n</attached_context>",
      'x<attached_context>\n<code_selection path="a.ts" lines="1" />\n</attached_context>',
      '<attached_context>\n<code_selection path="a.ts" lines="9-2" />\n</attached_context>',
      '<attached_context>\n<review_comment path="a.ts" change="moved">\n> x\n\ny\n</review_comment>\n</attached_context>',
      "<attached_context>\n<quoted_text>\nnot quoted\n</quoted_text>\n</attached_context>",
    ]) {
      expect(splitChatContext(message)).toEqual({ text: message, items: [] });
    }
  });
});

describe("quoteContext", () => {
  it("normalizes line endings and ignores empty selections", () => {
    expect(quoteContext(" one\r\ntwo ")).toEqual({
      kind: "quote",
      text: "one\ntwo",
    });
    expect(quoteContext(" \n ")).toBeNull();
  });
});

describe("addChatContext", () => {
  it("skips an item that is already attached", () => {
    const items = addChatContext([code], { ...code });
    expect(items).toEqual([code]);
    expect(addChatContext(items, quote)).toEqual([code, quote]);
  });
});

describe("chatContextLabel", () => {
  it("names each item on one line", () => {
    expect(chatContextLabel(quote)).toBe("line one");
    expect(chatContextLabel(code)).toBe("App.tsx:12-40");
    expect(chatContextLabel(comment)).toBe(
      "auth.ts:42 Handle a missing cookie.",
    );
    expect(chatContextSummary([code, quote, comment])).toBe("App.tsx:12-40 +2");
  });
});

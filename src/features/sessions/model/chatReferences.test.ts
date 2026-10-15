import { describe, expect, it } from "vitest";
import {
  chatReferencesLabel,
  composeWithChatReferences,
  newChatReference,
  removeChatReference,
  updateChatReferenceNote,
} from "./chatReferences";

describe("chat references", () => {
  it("labels the count", () => {
    expect(chatReferencesLabel(1)).toBe("1 annotation");
    expect(chatReferencesLabel(3)).toBe("3 annotations");
  });

  it("edits and removes one reference without touching the others", () => {
    const a = newChatReference("first", "");
    const b = newChatReference("second", "keep");
    const edited = updateChatReferenceNote([a, b], a.id, "  fix this \r\n");
    expect(edited.map((item) => item.note)).toEqual(["fix this", "keep"]);
    expect(removeChatReference(edited, a.id)).toEqual([b]);
  });

  it("appends every cited passage and its note after the message", () => {
    const refs = [
      newChatReference("line one\n\nline two", "this is wrong"),
      newChatReference("another", ""),
    ];
    expect(composeWithChatReferences("Please fix\n", refs)).toBe(
      [
        "Please fix",
        "Reference 1:\n> line one\n>\n> line two\n\nNote: this is wrong",
        "Reference 2:\n> another",
      ].join("\n\n"),
    );
  });

  it("sends references alone when there is no message text", () => {
    expect(
      composeWithChatReferences("", [newChatReference("only", "why?")]),
    ).toBe("Reference 1:\n> only\n\nNote: why?");
  });

  it("leaves the message alone when nothing is cited", () => {
    expect(composeWithChatReferences("hello ", [])).toBe("hello ");
  });

  it("names the document passage a reference came from", () => {
    const reference = newChatReference("const x = 1;", "why?", undefined, {
      label: "@src/app.ts (lines 3-3)",
      path: "src/app.ts",
      from: 0,
      to: 12,
    });
    expect(composeWithChatReferences("", [reference])).toBe(
      "Reference 1 — @src/app.ts (lines 3-3):\n> const x = 1;\n\nNote: why?",
    );
  });
});

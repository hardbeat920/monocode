import { describe, expect, it } from "vitest";
import { joinStreamText, snapshotRemainder, streamTextDelta } from "./streamText";

describe("joinStreamText", () => {
  it("appends tokens, including doubled letters and punctuation", () => {
    expect(joinStreamText("book", "keeper")).toBe("bookkeeper");
    expect(joinStreamText("Wait.", ". Next")).toBe("Wait.. Next");
  });

  it("keeps heading and paragraph on separate lines", () => {
    const chunks = ["# Result", "\n", "\n", "Here is the answer."];
    expect(chunks.reduce(joinStreamText, "")).toBe(
      "# Result\n\nHere is the answer.",
    );
  });

  it("keeps GFM table row boundaries", () => {
    const chunks = ["| a | b |\n", "\n", "| --- | --- |\n", "| 1 | 2 |"];
    expect(chunks.reduce(joinStreamText, "")).toBe(
      "| a | b |\n\n| --- | --- |\n| 1 | 2 |",
    );
  });

  it("does not drop a pipe that starts the next table row", () => {
    expect(joinStreamText("| a | b |\n", "|")).toBe("| a | b |\n|");
  });

  it("accepts a growing snapshot without doubling", () => {
    expect(joinStreamText("hel", "hello")).toBe("hello");
    expect(joinStreamText("hello", "hello")).toBe("hello");
  });

  it("keeps trailing newlines when a snapshot trims them", () => {
    expect(joinStreamText("hello\n\n", "hello")).toBe("hello\n\n");
  });
});

describe("snapshotRemainder", () => {
  it("skips a completed copy of text that already streamed", () => {
    expect(snapshotRemainder("hello", "hello")).toBe("");
    expect(snapshotRemainder("hello\n\n", "hello")).toBe("");
  });

  it("emits only the missing suffix", () => {
    expect(snapshotRemainder("hel", "hello")).toBe("lo");
  });

  it("emits a later stretch after a tool instead of pasting the first reply again", () => {
    expect(snapshotRemainder("I'll read the file", "Here's what I found")).toBe(
      "Here's what I found",
    );
  });

  it("does not paste an earlier snapshot once later text has already streamed", () => {
    expect(
      snapshotRemainder(
        "I'll read the fileHere's what I found",
        "I'll read the file",
      ),
    ).toBe("");
  });
});

describe("streamTextDelta", () => {
  it("keeps whitespace-only body text", () => {
    expect(streamTextDelta("\n\n")).toBe("\n\n");
    expect(streamTextDelta("  ")).toBe("  ");
    expect(streamTextDelta("")).toBe("");
    expect(streamTextDelta(undefined)).toBe("");
  });
});

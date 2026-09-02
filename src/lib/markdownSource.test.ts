import { describe, expect, it } from "vitest";
import { isAtxHeadingLine, normalizeFileLinks } from "./markdownSource";

describe("isAtxHeadingLine", () => {
  it("matches ATX headings", () => {
    expect(isAtxHeadingLine("# Title")).toBe(true);
    expect(isAtxHeadingLine("## Agent OS – Project Overview")).toBe(true);
    expect(isAtxHeadingLine("### What exists today")).toBe(true);
    expect(isAtxHeadingLine("###### Deep")).toBe(true);
  });

  it("allows up to three leading spaces", () => {
    expect(isAtxHeadingLine("   ## Indented")).toBe(true);
    expect(isAtxHeadingLine("    ## Too deep")).toBe(false);
  });

  it("rejects hashes that are not headings", () => {
    expect(isAtxHeadingLine("Not a heading")).toBe(false);
    expect(isAtxHeadingLine("#hashtag")).toBe(false);
    expect(isAtxHeadingLine("Text # not a heading")).toBe(false);
    expect(isAtxHeadingLine("####### seven")).toBe(false);
  });
});

describe("normalizeFileLinks", () => {
  it("converts file:// URI links to direct path links", () => {
    expect(
      normalizeFileLinks(
        "Check [protocol](file:///Users/dev/project/src/lib/protocol.ts) for details.",
      ),
    ).toBe(
      "Check [protocol](/Users/dev/project/src/lib/protocol.ts) for details.",
    );
  });

  it("preserves line range hashes and code inside link text", () => {
    expect(
      normalizeFileLinks(
        "See [`buildArgs()`](file:///Users/dev/project/src/lib/protocol.ts#L41-L81).",
      ),
    ).toBe(
      "See [`buildArgs()`](/Users/dev/project/src/lib/protocol.ts#L41-L81).",
    );
  });

  it("handles angle bracket URLs and URL-encoded characters", () => {
    expect(
      normalizeFileLinks(
        "File: [readme](<file:///Users/dev/my%20app/README.md#L5>)",
      ),
    ).toBe("File: [readme](/Users/dev/my app/README.md#L5)");

    expect(
      normalizeFileLinks("<file:///Users/dev/my%20app/README.md#L10>"),
    ).toBe("</Users/dev/my app/README.md#L10>");
  });

  it("handles Windows drive letter file URIs", () => {
    expect(
      normalizeFileLinks(
        "See [win](file:///C:/Users/dev/project/src/main.rs#L10).",
      ),
    ).toBe("See [win](C:/Users/dev/project/src/main.rs#L10).");

    expect(
      normalizeFileLinks("See [win](file://C:/Users/dev/project/src/main.rs)."),
    ).toBe("See [win](C:/Users/dev/project/src/main.rs).");
  });

  it("leaves non-file links untouched", () => {
    expect(
      normalizeFileLinks("Visit [Google](https://google.com) or `/local/path`"),
    ).toBe("Visit [Google](https://google.com) or `/local/path`");
  });
});


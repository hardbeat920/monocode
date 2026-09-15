import { describe, expect, it } from "vitest";
import { parseStandaloneHttpUrl, parseUserMessageLink } from "./linkPreview";

describe("parseStandaloneHttpUrl", () => {
  it("normalizes a standalone web URL for display", () => {
    expect(
      parseStandaloneHttpUrl(
        "  https://www.example.com/docs/start?q=one#intro  ",
      ),
    ).toEqual({
      url: "https://www.example.com/docs/start?q=one#intro",
      host: "example.com",
      displayUrl: "example.com/docs/start?q=one#intro",
    });
  });

  it("keeps a root URL compact", () => {
    expect(parseStandaloneHttpUrl("http://example.com/")?.displayUrl).toBe(
      "example.com",
    );
  });

  it("does not turn prose or credentialed URLs into preview cards", () => {
    expect(
      parseStandaloneHttpUrl("take a look at https://example.com"),
    ).toBeNull();
    expect(
      parseStandaloneHttpUrl("https://person:secret@example.com"),
    ).toBeNull();
    expect(parseStandaloneHttpUrl("file:///tmp/example.html")).toBeNull();
  });
});

describe("parseUserMessageLink", () => {
  it("extracts a URL followed by a comment", () => {
    expect(
      parseUserMessageLink(
        "https://github.com/hardbeat920/monocode/pull/226 check this",
      ),
    ).toEqual({
      link: {
        url: "https://github.com/hardbeat920/monocode/pull/226",
        host: "github.com",
        displayUrl: "github.com/hardbeat920/monocode/pull/226",
      },
      beforeText: "",
      afterText: " check this",
    });
  });

  it("preserves prose around a URL and drops sentence punctuation", () => {
    const result = parseUserMessageLink(
      "Please review (https://example.com/docs), thanks",
    );
    expect(result?.beforeText).toBe("Please review (");
    expect(result?.afterText).toBe("), thanks");
    expect(result?.link.url).toBe("https://example.com/docs");
  });

  it("returns null when there is no valid web URL", () => {
    expect(parseUserMessageLink("Nothing to preview here")).toBeNull();
  });
});

import { describe, expect, it } from "vitest";
import { parseGithubRef, truncateWords } from "./githubLinks";

describe("truncateWords", () => {
  it("leaves short titles untouched", () => {
    expect(truncateWords("Fix the bug", 6)).toBe("Fix the bug");
  });

  it("clamps long titles and appends an ellipsis", () => {
    expect(
      truncateWords("Mobile: paste a copied image into the composer", 6),
    ).toBe("Mobile: paste a copied image into…");
  });

  it("collapses surrounding whitespace", () => {
    expect(truncateWords("  spaced   out  ", 6)).toBe("spaced out");
  });
});

describe("parseGithubRef", () => {
  it("recognises pull request links", () => {
    expect(parseGithubRef("https://github.com/hardbeat920/monocode/pull/1343"))
      .toEqual({ kind: "pr", repo: "hardbeat920/monocode", label: "#1343" });
  });

  it("recognises issue links", () => {
    expect(parseGithubRef("https://github.com/acme/widgets/issues/7")).toEqual({
      kind: "issue",
      repo: "acme/widgets",
      label: "#7",
    });
  });

  it("shortens commit SHAs to seven characters", () => {
    expect(
      parseGithubRef(
        "https://github.com/acme/widgets/commit/0123456789abcdef0123456789abcdef01234567",
      ),
    ).toEqual({ kind: "commit", repo: "acme/widgets", label: "0123456" });
  });

  it("tolerates a trailing slash and the www host", () => {
    expect(parseGithubRef("https://www.github.com/a/b/pull/9/")).toEqual({
      kind: "pr",
      repo: "a/b",
      label: "#9",
    });
  });

  it("ignores non-GitHub and unrelated GitHub links", () => {
    expect(parseGithubRef("https://example.com/a/b/pull/1")).toBeNull();
    expect(parseGithubRef("https://github.com/acme/widgets")).toBeNull();
    expect(parseGithubRef("https://github.com/acme/widgets/tree/main")).toBeNull();
    expect(parseGithubRef("not a url")).toBeNull();
    expect(parseGithubRef(undefined)).toBeNull();
  });

  it("requires commit refs to look like SHAs", () => {
    expect(
      parseGithubRef("https://github.com/acme/widgets/commit/not-a-sha"),
    ).toBeNull();
  });
});

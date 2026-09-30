import { describe, expect, it } from "vitest";
import {
  claudeProjectDir,
  claudeTranscriptPath,
  encodeProjectDir,
} from "./transcriptPath";

const HOME = "/Users/me";

describe("encodeProjectDir", () => {
  // These are the cases from `claude_project_dir`'s own tests in
  // `src-tauri/src/fs.rs`. If Rust changes, these must fail.
  it.each([
    ["C:\\Users\\dev\\proj", "C--Users-dev-proj"],
    ["/Users/me/çalışma", "-Users-me--al--ma"],
    ["/Users/me/My Project (v2)", "-Users-me-My-Project--v2-"],
    ["/a/b.c", "-a-b-c"],
    ["/a/b_c", "-a-b-c"],
  ])("encodes %j as %j", (cwd, expected) => {
    expect(encodeProjectDir(cwd)).toBe(expected);
  });

  it("collapses dotted, dashed and underscored paths together", () => {
    // The Rust test asserts these three are the same directory.
    expect(encodeProjectDir("/a/b.c")).toBe(encodeProjectDir("/a/b-c"));
    expect(encodeProjectDir("/a/b.c")).toBe(encodeProjectDir("/a/b_c"));
  });

  it("spends one dash per code point, not per UTF-16 unit", () => {
    // Rust maps over `chars()`. An astral character is a single char there, so
    // a per-unit implementation would emit two dashes and miss the directory.
    expect(encodeProjectDir("/a/\u{1F600}b")).toBe("-a--b");
  });

  it("keeps ASCII letters and digits and nothing else", () => {
    expect(encodeProjectDir("aZ09")).toBe("aZ09");
    expect(encodeProjectDir("é")).toBe("-");
    expect(encodeProjectDir(" ")).toBe("-");
  });

  it("returns an empty name for an empty path", () => {
    expect(encodeProjectDir("")).toBe("");
  });
});

describe("claudeTranscriptPath", () => {
  it("builds the transcript path under the encoded project directory", () => {
    expect(claudeTranscriptPath(HOME, "/Users/me/proj", "sess-1")).toBe(
      "/Users/me/.claude/projects/-Users-me-proj/sess-1.jsonl",
    );
  });

  it("does not double the separator when home has a trailing slash", () => {
    expect(claudeTranscriptPath("/Users/me/", "/a", "s")).toBe(
      "/Users/me/.claude/projects/-a/s.jsonl",
    );
  });

  it("puts the transcript inside the project directory it reports", () => {
    const dir = claudeProjectDir(HOME, "/a/b");
    expect(claudeTranscriptPath(HOME, "/a/b", "s")).toBe(`${dir}/s.jsonl`);
  });
});

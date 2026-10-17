import { describe, expect, it } from "vitest";
import type { FsEntry } from "../../../platform/tauri/fs";
import {
  pathMentionDir,
  pathMentionQuery,
  rankPathEntries,
} from "./pathMentions";

const entry = (name: string, isDir = false, ignored = false): FsEntry => ({
  name,
  path: `/work/${name}`,
  isDir,
  ignored,
});

describe("pathMentionQuery", () => {
  it.each([
    ["../", { dir: "../", partial: "" }],
    ["../mono", { dir: "../", partial: "mono" }],
    ["../../a/b", { dir: "../../a/", partial: "b" }],
    ["~/", { dir: "~/", partial: "" }],
    ["/Users/me/x", { dir: "/Users/me/", partial: "x" }],
  ])("splits %s", (query, expected) => {
    expect(pathMentionQuery(query)).toEqual(expected);
  });

  it.each(["", "src/App", "..", "~", "./src", ".env"])(
    "leaves %s to the project index",
    (query) => {
      expect(pathMentionQuery(query)).toBeNull();
    },
  );
});

describe("pathMentionDir", () => {
  it("joins relative parents onto the cwd", () => {
    expect(pathMentionDir("../", "/work/repo")).toBe("/work");
    expect(pathMentionDir("../other/", "/work/repo")).toBe("/work/other");
  });

  it("passes absolute and home paths through", () => {
    expect(pathMentionDir("/etc/", "/work/repo")).toBe("/etc/");
    expect(pathMentionDir("~/notes/", "")).toBe("~/notes/");
  });

  it("needs a cwd for relative paths", () => {
    expect(pathMentionDir("../", "")).toBeNull();
  });
});

describe("rankPathEntries", () => {
  const entries = [
    entry("api", true),
    entry("monocode", true),
    entry(".config", true),
    entry("My Docs", true),
    entry("readme.md"),
  ];

  it("labels entries with the typed directory and keeps list order", () => {
    const ranked = rankPathEntries({ dir: "../", partial: "" }, entries);
    expect(ranked.map((file) => file.relative)).toEqual([
      "../api",
      "../monocode",
      "../readme.md",
    ]);
    expect(ranked[0]).toMatchObject({ path: "/work/api", isDir: true });
  });

  it("fuzzy-filters and offsets match positions past the directory", () => {
    const ranked = rankPathEntries({ dir: "../", partial: "mono" }, entries);
    expect(ranked.map((file) => file.relative)).toEqual(["../monocode"]);
    expect(ranked[0]!.positions).toEqual([3, 4, 5, 6]);
  });

  it("shows dotfiles only once a leading dot is typed", () => {
    const ranked = rankPathEntries({ dir: "~/", partial: ".co" }, entries);
    expect(ranked.map((file) => file.relative)).toEqual(["~/.config"]);
  });

  it("drops git-ignored entries below matching ones", () => {
    const ranked = rankPathEntries({ dir: "/", partial: "a" }, [
      entry("alpha", false, true),
      entry("abc"),
    ]);
    expect(ranked.map((file) => file.name)).toEqual(["abc", "alpha"]);
  });
});

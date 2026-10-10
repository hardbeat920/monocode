import { describe, expect, it } from "vitest";
import {
  inWorktreeFocus,
  setWorktreeFocus,
  worktreeFocus,
} from "./worktreeFocus";

describe("worktree focus", () => {
  it("keeps every session when nothing is focused", () => {
    expect(inWorktreeFocus({ cwd: "/repo" }, undefined)).toBe(true);
    expect(
      inWorktreeFocus({ cwd: "/repo", worktreeCwd: "/trees/a" }, undefined),
    ).toBe(true);
  });

  it("matches sessions by their working copy", () => {
    const main = { path: "/repo", branch: "main" };
    const feature = { path: "/trees/a", branch: "feat/a" };
    const inMain = { cwd: "/repo" };
    const inFeature = { cwd: "/repo", worktreeCwd: "/trees/a" };
    expect(inWorktreeFocus(inMain, main)).toBe(true);
    expect(inWorktreeFocus(inFeature, main)).toBe(false);
    expect(inWorktreeFocus(inFeature, feature)).toBe(true);
    expect(inWorktreeFocus(inMain, feature)).toBe(false);
  });

  it("stores focus per project", () => {
    setWorktreeFocus("/repo", { path: "/trees/a", branch: "feat/a" });
    expect(worktreeFocus("/repo")?.path).toBe("/trees/a");
    expect(worktreeFocus("/other")).toBeUndefined();
    setWorktreeFocus("/repo", undefined);
    expect(worktreeFocus("/repo")).toBeUndefined();
  });
});

it("keeps CoW session ownership while matching its execution path", () => {
  const focus = {
    path: "/copies/a",
    branch: "feature",
    cowId: "copy-a",
    sessionId: "owner",
  };
  setWorktreeFocus("/cow-project", focus);
  expect(worktreeFocus("/cow-project")).toEqual(focus);
  expect(
    inWorktreeFocus({ cwd: "/cow-project", worktreeCwd: "/copies/a" }, focus),
  ).toBe(true);
  expect(
    inWorktreeFocus({ cwd: "/cow-project", worktreeCwd: "/copies/b" }, focus),
  ).toBe(false);
  setWorktreeFocus("/cow-project", undefined);
});

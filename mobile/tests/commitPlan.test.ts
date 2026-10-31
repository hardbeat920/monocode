import { expect, it } from "vitest";
import { selection, shipStep, shipTitle, syncLabel } from "@/features/changes/commitPlan";
import type { GitStatus } from "@/features/connection/useProjectGit";

const status = (over: Partial<GitStatus> = {}): GitStatus => ({
  branch: "feature",
  files: 0,
  remote: true,
  upstream: true,
  defaultBranch: "main",
  ahead: 0,
  behind: 0,
  aheadOfDefault: 0,
  canGenerate: true,
  pr: null,
  ...over,
});

it("commits first, whatever the branch says", () => {
  expect(shipStep(3, null)).toBe("commit");
  expect(shipStep(212, status({ pr: { number: 1, url: "u" } }))).toBe("commit");
});

it("reads the next step from the branch", () => {
  expect(shipStep(0, null)).toBeNull();
  expect(shipStep(0, status({ remote: false }))).toBeNull();
  expect(shipStep(0, status({ ahead: 2, aheadOfDefault: 2 }))).toBe("push");
  expect(shipStep(0, status({ upstream: false, aheadOfDefault: 1 }))).toBe("push");
  expect(shipStep(0, status({ upstream: false }))).toBeNull();
  expect(shipStep(0, status({ aheadOfDefault: 2 }))).toBe("pr");
  expect(shipStep(0, status({ aheadOfDefault: 2, pr: { number: 7, url: "u" } }))).toBe("done");
  expect(shipStep(0, status())).toBeNull();
});

it("opens no pull request from the default branch or a diverged one", () => {
  expect(shipStep(0, status({ branch: "main", aheadOfDefault: 1 }))).toBeNull();
  expect(shipStep(0, status({ aheadOfDefault: 1, behind: 0, upstream: true }))).toBe("pr");
  expect(shipStep(0, status({ upstream: true, ahead: 1, behind: 1, aheadOfDefault: 1 }))).toBe("push");
});

it("says where the work stands", () => {
  expect(shipTitle("commit", 1, null)).toBe("1 file changed");
  expect(shipTitle("commit", 212, null)).toBe("212 files changed");
  expect(shipTitle("push", 0, status({ ahead: 4 }))).toBe("Committed · 4 ahead");
  expect(shipTitle("push", 0, status({ upstream: false }))).toBe("Committed · not pushed");
  expect(shipTitle("done", 0, status({ pr: { number: 482, url: "u" } }))).toBe("PR #482 open");
});

it("labels the push by what the branch has", () => {
  expect(syncLabel(status({ upstream: false }))).toBe("Publish Branch");
  expect(syncLabel(status({ ahead: 1 }))).toBe("Push 1 commit");
  expect(syncLabel(status({ ahead: 3 }))).toBe("Push 3 commits");
  expect(syncLabel(status({ behind: 2 }))).toBe("Pull 2 commits");
  expect(syncLabel(status({ ahead: 1, behind: 2 }))).toBe("Sync ↓2 ↑1");
});

it("totals only the commits kept", () => {
  const plan = [
    { message: "a", paths: ["1", "2"], additions: 10, deletions: 1 },
    { message: "b", paths: ["3"], additions: 5, deletions: 2 },
    { message: "c", paths: ["4", "5", "6"], additions: 1, deletions: 1 },
  ];
  const kept = selection(plan, [true, false, true]);
  expect(kept.commits.map((c) => c.message)).toEqual(["a", "c"]);
  expect(kept).toMatchObject({ files: 5, additions: 11, deletions: 2 });
  expect(selection(plan, [false, false, false]).commits).toEqual([]);
});

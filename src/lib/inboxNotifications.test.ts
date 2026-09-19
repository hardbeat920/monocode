import { expect, it } from "vitest";
import { InboxNotificationTracker } from "./inboxNotifications";
import type { InboxItem } from "./githubTasks";

function item(repo: string, updatedAt: string): InboxItem {
  return {
    provider: "github",
    kind: "pr",
    repo,
    number: 1,
    title: "PR",
    url: `https://github.com/${repo}/pull/1`,
    updatedAt,
    state: "open",
    labels: [],
    assignees: [],
    draft: false,
    projectPath: `/tmp/${repo}`,
  };
}

it("detects a new revision even while another project's activity remains unread", () => {
  const tracker = new InboxNotificationTracker();
  const a = item("acme/private", "2026-09-14T08:00:00Z");
  const b = item("acme/work", "2026-09-14T08:01:00Z");
  expect(tracker.observe([a], "all")).toEqual([]);
  expect(tracker.observe([a, b], "all")).toEqual([b]);
  expect(tracker.observe([a, b], "all")).toEqual([]);
  const updated = { ...b, updatedAt: "2026-09-14T08:02:00Z" };
  expect(tracker.observe([a, updated], "all")).toEqual([updated]);
});

it("waits for a successful provider baseline instead of ringing for recovered history", () => {
  const tracker = new InboxNotificationTracker();
  const a = item("acme/app", "2026-09-14T08:00:00Z");
  tracker.observe([], "all", ["github"]);
  expect(tracker.observe([a], "all")).toEqual([]);
  const changed = { ...a, updatedAt: "2026-09-14T08:01:00Z" };
  expect(tracker.observe([changed], "all")).toEqual([changed]);
});

it("does not announce history exposed by a changed query or repeat items after a failed or partial refresh", () => {
  const tracker = new InboxNotificationTracker();
  const a = item("acme/app", "2026-09-14T08:00:00Z");
  const history = { ...a, number: 2, updatedAt: "2026-08-01T08:00:00Z" };
  tracker.observe([a], "open");
  expect(tracker.observe([a, history], "all")).toEqual([]);
  expect(tracker.observe([], "all")).toEqual([]);
  expect(tracker.observe([a, history], "all")).toEqual([]);
  expect(tracker.observe([{ ...a, updatedAt: "invalid" }], "all")).toEqual([]);
});

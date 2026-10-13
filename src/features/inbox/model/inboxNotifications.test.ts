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
  expect(tracker.observe([a], "all")).toEqual({ changed: [], appeared: [] });
  expect(tracker.observe([a, b], "all")).toEqual({
    changed: [b],
    appeared: [b],
  });
  expect(tracker.observe([a, b], "all")).toEqual({ changed: [], appeared: [] });
  const updated = { ...b, updatedAt: "2026-09-14T08:02:00Z" };
  expect(tracker.observe([a, updated], "all")).toEqual({
    changed: [updated],
    appeared: [],
  });
});

it("waits for a successful provider baseline instead of ringing for recovered history", () => {
  const tracker = new InboxNotificationTracker();
  const a = item("acme/app", "2026-09-14T08:00:00Z");
  tracker.observe([], "all", ["github"]);
  expect(tracker.observe([a], "all")).toEqual({ changed: [], appeared: [] });
  const changed = { ...a, updatedAt: "2026-09-14T08:01:00Z" };
  expect(tracker.observe([changed], "all")).toEqual({
    changed: [changed],
    appeared: [],
  });
});

it("does not announce history exposed by a changed query or repeat items after a failed or partial refresh", () => {
  const tracker = new InboxNotificationTracker();
  const a = item("acme/app", "2026-09-14T08:00:00Z");
  const history = { ...a, number: 2, updatedAt: "2026-08-01T08:00:00Z" };
  tracker.observe([a], "open");
  expect(tracker.observe([a, history], "all")).toEqual({
    changed: [],
    appeared: [],
  });
  expect(tracker.observe([], "all")).toEqual({ changed: [], appeared: [] });
  expect(tracker.observe([a, history], "all")).toEqual({
    changed: [],
    appeared: [],
  });
  expect(tracker.observe([{ ...a, updatedAt: "invalid" }], "all")).toEqual({
    changed: [],
    appeared: [],
  });
});

it("keeps a stable identity across provider URL shapes", () => {
  const variants: Array<[string, InboxItem]> = [
    [
      "gitlab",
      {
        provider: "gitlab",
        kind: "mr",
        repo: "group/api",
        number: 7,
        title: "MR",
        url: "https://gitlab.example.com/group/api/-/merge_requests/7",
        updatedAt: "2026-09-14T08:00:00Z",
        state: "open",
        labels: [],
        assignees: [],
        draft: false,
      },
    ],
    [
      "azuredevops",
      {
        provider: "azuredevops",
        kind: "pr",
        repo: "org/proj/repo",
        number: 3,
        title: "PR",
        url: "https://dev.azure.com/org/proj/_git/repo/pullrequest/3",
        updatedAt: "2026-09-14T08:00:00Z",
        state: "open",
        labels: [],
        assignees: [],
        draft: false,
      },
    ],
  ];
  for (const [name, first] of variants) {
    const tracker = new InboxNotificationTracker();
    tracker.observe([first], `${name}-one`);
    expect(tracker.observe([{ ...first }], `${name}-two`).appeared, name).toEqual([]);
  }
  // Jira/Linear key on team/project ids; unparseable URLs fall back to repo.
  const tracker = new InboxNotificationTracker();
  const jira: InboxItem = {
    provider: "jira",
    kind: "issue",
    repo: "ENG",
    id: "10042",
    identifier: "ENG-2",
    title: "Ticket",
    url: "https://acme.atlassian.net/browse/ENG-2",
    updatedAt: "2026-09-14T08:00:00Z",
    state: "open",
    labels: [],
    assignees: [],
    draft: false,
  };
  const badUrl = { ...item("acme/web", "2026-09-14T08:00:00Z"), kind: "issue" as const, number: 5, url: "not-a-url" };
  tracker.observe([jira, badUrl], "one");
  expect(tracker.observe([jira, badUrl], "two").appeared).toEqual([]);
});

it("does not replay open items when the local project scope changes", () => {
  const tracker = new InboxNotificationTracker();
  const a = item("acme/app", "2026-09-14T08:00:00Z");
  tracker.observe([a], "/tmp/app");
  // Same PR observed under a different scope (project added, path moved).
  const moved = { ...a, projectPath: "/elsewhere/app" };
  expect(tracker.observe([moved], "/elsewhere/app")).toEqual({
    changed: [],
    appeared: [],
  });
  // A genuinely new PR in the same repo still appears.
  const fresh = { ...moved, number: 2, updatedAt: "2026-09-14T09:00:00Z" };
  expect(tracker.observe([moved, fresh], "/elsewhere/app")).toEqual({
    changed: [fresh],
    appeared: [fresh],
  });
});

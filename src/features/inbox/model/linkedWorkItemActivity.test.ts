import { describe, expect, it } from "vitest";
import type { GithubWorkItemThread } from "./githubTasks";
import {
  completeLinkedWorkItemUpdateCard,
  linkedWorkItemActivityPrompt,
  linkedWorkItemTerminalState,
  linkedWorkItemUpdateSummary,
  pendingLinkedWorkItemUpdateCard,
} from "./linkedWorkItemActivity";
import type { LinkedSessionUpdate } from "./linkedSessionUpdates";

const update: LinkedSessionUpdate = {
  sessionId: "session-1",
  since: Date.parse("2026-09-13T10:00:00Z"),
  updatedAt: Date.parse("2026-09-13T12:00:00Z"),
  item: {
    kind: "pr",
    repo: "acme/app",
    number: 42,
    title: "Update sidebar activity",
    url: "https://github.com/acme/app/pull/42",
    state: "open",
    updatedAt: "2026-09-13T12:00:00Z",
    labels: [],
    assignees: [],
    draft: false,
  },
};

const thread: GithubWorkItemThread = {
  comments: [
    {
      id: "old",
      kind: "comment",
      author: "old-user",
      body: "Already handled",
      createdAt: "2026-09-13T09:00:00Z",
      url: "",
      state: "",
      path: "",
      line: null,
      resolved: false,
      threadId: "",
      replies: [],
    },
    {
      id: "review",
      kind: "review",
      author: "maya",
      body: "Please cover the empty state",
      createdAt: "2026-09-13T11:30:00Z",
      url: "https://github.com/acme/app/pull/42#review",
      state: "CHANGES_REQUESTED",
      path: "",
      line: null,
      resolved: false,
      threadId: "",
      replies: [],
    },
  ],
  commits: [
    {
      oid: "abcdef123456",
      messageHeadline: "Handle linked activity",
      author: "nik",
      committedDate: "2026-09-13T11:00:00Z",
      url: "https://github.com/acme/app/commit/abcdef123456",
    },
  ],
  truncated: false,
  reviewDecision: "CHANGES_REQUESTED",
  baseRefName: "main",
  headRefName: "activity",
};

describe("linked work item activity card", () => {
  it("only includes activity newer than the prior read baseline", () => {
    const card = completeLinkedWorkItemUpdateCard(
      pendingLinkedWorkItemUpdateCard(update),
      thread,
    );

    expect(card.counts).toEqual({ comments: 0, reviews: 1, commits: 1 });
    expect(card.entries.map((entry) => entry.id)).toEqual([
      "review",
      "abcdef123456",
    ]);
    expect(linkedWorkItemUpdateSummary(card)).toBe(
      "1 new commit · 1 new review",
    );
  });

  it("builds an explicit agent action from the update details", () => {
    const card = completeLinkedWorkItemUpdateCard(
      pendingLinkedWorkItemUpdateCard(update),
      thread,
    );
    const message = linkedWorkItemActivityPrompt(card);

    expect(message).toContain("Address the new feedback");
    expect(message).toContain(
      "The linked GitHub pull request has new activity",
    );
    expect(message).toContain("review by @maya: Requested changes");
    expect(message).toContain("commit by @nik: Handle linked activity");
  });

  it("recognizes terminal issue and pull request states", () => {
    expect(
      linkedWorkItemTerminalState({ kind: "issue", state: "CLOSED" }),
    ).toBe("issue_closed");
    expect(linkedWorkItemTerminalState({ kind: "pr", state: "merged" })).toBe(
      "pr_merged",
    );
    expect(linkedWorkItemTerminalState({ kind: "pr", state: "closed" })).toBe(
      "pr_closed",
    );
    expect(linkedWorkItemTerminalState({ kind: "issue", state: "open" })).toBe(
      undefined,
    );
  });
});

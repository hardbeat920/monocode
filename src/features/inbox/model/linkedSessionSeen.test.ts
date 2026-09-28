// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
import type { LinkedWorkItem } from "../../sessions/model/session";
import type { GithubWorkItem } from "./githubTasks";
import type { SessionSummary } from "../../sessions/data/sessionStore";
import {
  linkedSessionSeenAt,
  markLinkedSessionUpdateSeen,
} from "./linkedSessionSeen";
import {
  linkedSessionUpdates,
  linkedWorkItemUpdateKey,
} from "./linkedSessionUpdates";

const pull = { kind: "pr" as const, repo: "acme/app", number: 42 };
const issue = { kind: "issue" as const, repo: "acme/app", number: 8 };

describe("linked session seen snapshots", () => {
  beforeEach(() => localStorage.clear());

  it("remembers the newest acknowledged remote update per linked item", () => {
    markLinkedSessionUpdateSeen("session-1", pull, 200);
    markLinkedSessionUpdateSeen("session-1", pull, 150);
    markLinkedSessionUpdateSeen("session-1", issue, 180);
    markLinkedSessionUpdateSeen("session-2", pull, 300);

    expect(linkedSessionSeenAt("session-1", pull)).toBe(200);
    expect(linkedSessionSeenAt("session-1", issue)).toBe(180);
    expect(linkedSessionSeenAt("session-2", pull)).toBe(300);
  });

  it("reuses a legacy session acknowledgement only for the original linked item", () => {
    localStorage.setItem(
      "monocode.linkedSessionSeen",
      JSON.stringify({ "session-1": 200 }),
    );

    expect(linkedSessionSeenAt("session-1", pull, pull)).toBe(200);
    expect(linkedSessionSeenAt("session-1", issue, pull)).toBe(0);
    expect(linkedSessionSeenAt("session-1", pull)).toBe(0);
  });

  it("prefers the per-item acknowledgement over a legacy session stamp", () => {
    localStorage.setItem(
      "monocode.linkedSessionSeen",
      JSON.stringify({
        "session-1": 200,
        "session-1::acme/app:pr:42": 250,
      }),
    );

    expect(linkedSessionSeenAt("session-1", pull, pull)).toBe(250);
    expect(linkedSessionSeenAt("session-1", issue, pull)).toBe(0);
  });

  it("does not hide a later linked item behind a legacy session stamp", () => {
    const pullItem: LinkedWorkItem = {
      kind: "pr",
      repo: "acme/app",
      number: 42,
      url: "https://github.com/acme/app/pull/42",
    };
    const issueItem: LinkedWorkItem = {
      kind: "issue",
      repo: "acme/app",
      number: 8,
      url: "https://github.com/acme/app/issues/8",
    };
    const workItem = (
      item: LinkedWorkItem,
      updatedAt: number,
    ): GithubWorkItem => ({
      ...item,
      title: item.kind === "pr" ? "Pull" : "Issue",
      state: "open",
      updatedAt: new Date(updatedAt).toISOString(),
      labels: [],
      assignees: [],
      draft: false,
    });
    const session: SessionSummary = {
      id: "session-1",
      cwd: "/tmp/app",
      harness: "codex",
      model: "gpt-5",
      runtimeMode: "supervised",
      title: "codex · session-1",
      createdAt: 1,
      updatedAt: 100,
      linkedWorkItem: pullItem,
      linkedWorkItems: [pullItem, issueItem],
    };
    localStorage.setItem(
      "monocode.linkedSessionSeen",
      JSON.stringify({ "session-1": 200 }),
    );

    const updates = linkedSessionUpdates(
      [session],
      new Map([
        [linkedWorkItemUpdateKey(pullItem), workItem(pullItem, 200)],
        [linkedWorkItemUpdateKey(issueItem), workItem(issueItem, 180)],
      ]),
      linkedSessionSeenAt,
    );

    expect(updates.get("session-1")?.item.number).toBe(8);
    expect(updates.get("session-1")?.updatedAt).toBe(180);
  });
});

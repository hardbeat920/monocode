import { describe, expect, it } from "vitest";
import type { LinkedWorkItem } from "../../sessions/model/session";
import type { GithubWorkItem } from "./githubTasks";
import type { SessionSummary } from "../../sessions/data/sessionStore";
import {
  linkedSessionUpdateIds,
  linkedSessionUpdates,
  linkedSessionUpdatesToReveal,
  linkedWorkItemActivityKey,
  linkedWorkItemTargets,
  linkedWorkItemUpdateKey,
  sameLinkedWorkItemActivity,
} from "./linkedSessionUpdates";

const linked: LinkedWorkItem = {
  kind: "pr",
  repo: "Acme/App",
  number: 42,
  url: "https://github.com/Acme/App/pull/42",
};

function remote(updatedAt: number): GithubWorkItem {
  return {
    ...linked,
    title: "Update sidebar activity",
    state: "open",
    updatedAt: new Date(updatedAt).toISOString(),
    labels: [],
    assignees: [],
    draft: false,
  };
}

function session(
  id: string,
  updatedAt: number,
  overrides: Partial<SessionSummary> = {},
): SessionSummary {
  return {
    id,
    cwd: "/tmp/app",
    harness: "codex",
    model: "gpt-5",
    runtimeMode: "supervised",
    title: `codex · ${id}`,
    createdAt: 1,
    updatedAt,
    linkedWorkItem: linked,
    ...overrides,
  };
}

describe("linked session updates", () => {
  it("marks a session when its linked item changed after the local session", () => {
    const snapshots = new Map([[linkedWorkItemUpdateKey(linked), remote(200)]]);
    expect([
      ...linkedSessionUpdateIds([session("old", 100)], snapshots),
    ]).toEqual(["old"]);
  });

  it("clears naturally once the session advances past the remote update", () => {
    const snapshots = new Map([[linkedWorkItemUpdateKey(linked), remote(200)]]);
    expect(
      linkedSessionUpdateIds([session("continued", 201)], snapshots).size,
    ).toBe(0);
  });

  it("tracks related sessions independently and ignores archived sessions", () => {
    const snapshots = new Map([[linkedWorkItemUpdateKey(linked), remote(200)]]);
    const ids = linkedSessionUpdateIds(
      [
        session("stale", 100),
        session("current", 250),
        session("archived", 100, { archived: true }),
        session("unlinked", 100, { linkedWorkItem: undefined }),
      ],
      snapshots,
    );
    expect([...ids]).toEqual(["stale"]);
  });

  it("normalizes repository case and deduplicates lookup targets", () => {
    const lower = { ...linked, repo: "acme/app" };
    const snapshots = new Map([[linkedWorkItemUpdateKey(lower), remote(200)]]);
    expect(
      linkedSessionUpdateIds([session("same", 100)], snapshots).has("same"),
    ).toBe(true);
    expect(
      linkedWorkItemTargets([
        session("first", 100),
        session("second", 150, { linkedWorkItem: lower }),
      ]),
    ).toHaveLength(1);
  });

  it("marks a session when a second linked item changes", () => {
    const issue: LinkedWorkItem = {
      kind: "issue",
      repo: "acme/app",
      number: 8,
      url: "https://github.com/acme/app/issues/8",
    };
    const snapshots = new Map([
      [
        linkedWorkItemUpdateKey(issue),
        {
          ...issue,
          title: "Follow-up",
          state: "open",
          updatedAt: new Date(200).toISOString(),
          labels: [],
          assignees: [],
          draft: false,
        },
      ],
    ]);
    expect(
      linkedSessionUpdateIds(
        [
          session("multi", 100, {
            linkedWorkItems: [linked, issue],
          }),
        ],
        snapshots,
      ).has("multi"),
    ).toBe(true);
  });

  it("uses the acknowledged snapshot as the next activity baseline", () => {
    const snapshots = new Map([[linkedWorkItemUpdateKey(linked), remote(200)]]);
    expect(
      linkedSessionUpdateIds([session("read", 100)], snapshots, () => 200).size,
    ).toBe(0);
    expect(
      linkedSessionUpdateIds([session("newer", 100)], snapshots, () => 150).has(
        "newer",
      ),
    ).toBe(true);
  });

  it("keeps a second linked-item update after the newest one is acknowledged", () => {
    const issue: LinkedWorkItem = {
      kind: "issue",
      repo: "acme/app",
      number: 8,
      url: "https://github.com/acme/app/issues/8",
    };
    const snapshots = new Map([
      [linkedWorkItemUpdateKey(linked), remote(200)],
      [
        linkedWorkItemUpdateKey(issue),
        {
          ...issue,
          title: "Follow-up",
          state: "open",
          updatedAt: new Date(180).toISOString(),
          labels: [],
          assignees: [],
          draft: false,
        },
      ],
    ]);
    const acknowledged = new Map<string, number>();
    const seenAt = (
      sessionId: string,
      item: Pick<LinkedWorkItem, "repo" | "kind" | "number">,
    ) => acknowledged.get(`${sessionId}:${linkedWorkItemUpdateKey(item)}`) ?? 0;
    const sessions = [
      session("multi", 100, { linkedWorkItems: [linked, issue] }),
    ];

    const first = linkedSessionUpdates(sessions, snapshots, seenAt);
    expect(first.get("multi")?.item.number).toBe(42);
    expect(
      linkedSessionUpdateIds(sessions, snapshots, seenAt).has("multi"),
    ).toBe(true);

    acknowledged.set(`multi:${linkedWorkItemUpdateKey(linked)}`, 200);
    const second = linkedSessionUpdates(sessions, snapshots, seenAt);
    expect(second.get("multi")?.item.number).toBe(8);
    expect(second.get("multi")?.updatedAt).toBe(180);

    acknowledged.set(`multi:${linkedWorkItemUpdateKey(issue)}`, 180);
    expect(linkedSessionUpdateIds(sessions, snapshots, seenAt).size).toBe(0);
  });

  it("passes the original linkedWorkItem into seenAt", () => {
    const issue: LinkedWorkItem = {
      kind: "issue",
      repo: "acme/app",
      number: 8,
      url: "https://github.com/acme/app/issues/8",
    };
    const seen: Array<{
      item: number;
      primary?: number;
    }> = [];
    linkedSessionUpdates(
      [session("multi", 100, { linkedWorkItems: [linked, issue] })],
      new Map([
        [linkedWorkItemUpdateKey(linked), remote(200)],
        [
          linkedWorkItemUpdateKey(issue),
          {
            ...issue,
            title: "Follow-up",
            state: "open",
            updatedAt: new Date(180).toISOString(),
            labels: [],
            assignees: [],
            draft: false,
          },
        ],
      ]),
      (_sessionId, item, primaryItem) => {
        seen.push({ item: item.number, primary: primaryItem?.number });
        return 0;
      },
    );
    expect(seen).toEqual([
      { item: 42, primary: 42 },
      { item: 8, primary: 42 },
    ]);
  });
});

describe("linked session updates to reveal", () => {
  const pullUpdate = {
    sessionId: "open",
    item: remote(200),
    since: 100,
    updatedAt: 200,
  };
  const issue: LinkedWorkItem = {
    kind: "issue",
    repo: "acme/app",
    number: 8,
    url: "https://github.com/acme/app/issues/8",
  };
  const issueUpdate = {
    sessionId: "open",
    item: {
      ...issue,
      title: "Follow-up",
      state: "open",
      updatedAt: new Date(180).toISOString(),
      labels: [],
      assignees: [],
      draft: false,
    },
    since: 100,
    updatedAt: 180,
  };
  const pullKey = `${linkedWorkItemUpdateKey(linked)}:200`;
  const issueKey = `${linkedWorkItemUpdateKey(issue)}:180`;

  it("does not reveal the first time an open session is observed", () => {
    const { reveal, selectionKeys } = linkedSessionUpdatesToReveal(
      ["open"],
      new Map([["open", pullUpdate]]),
      new Map(),
    );
    expect(reveal).toEqual([]);
    expect(selectionKeys.get("open")).toBe(pullKey);
  });

  it("reveals when the selected linked item changes", () => {
    const { reveal, selectionKeys } = linkedSessionUpdatesToReveal(
      ["open"],
      new Map([["open", issueUpdate]]),
      new Map([["open", pullKey]]),
    );
    expect(reveal).toEqual([issueUpdate]);
    expect(selectionKeys.get("open")).toBe(issueKey);
  });

  it("does not reveal when dismissal leaves the same selected update", () => {
    const { reveal } = linkedSessionUpdatesToReveal(
      ["open"],
      new Map([["open", pullUpdate]]),
      new Map([["open", pullKey]]),
    );
    expect(reveal).toEqual([]);
  });

  it("does not reveal when acknowledgement clears the selected update", () => {
    const { reveal, selectionKeys } = linkedSessionUpdatesToReveal(
      ["open"],
      new Map(),
      new Map([["open", pullKey]]),
    );
    expect(reveal).toEqual([]);
    expect(selectionKeys.get("open")).toBe("");
  });

  it("reveals when an already-open session gains a selected update", () => {
    const { reveal } = linkedSessionUpdatesToReveal(
      ["open"],
      new Map([["open", pullUpdate]]),
      new Map([["open", ""]]),
    );
    expect(reveal).toEqual([pullUpdate]);
  });

  it("treats two linked items with the same timestamp as different selections", () => {
    const otherUpdate = {
      ...issueUpdate,
      updatedAt: pullUpdate.updatedAt,
    };
    const { reveal } = linkedSessionUpdatesToReveal(
      ["open"],
      new Map([["open", otherUpdate]]),
      new Map([["open", pullKey]]),
    );
    expect(reveal).toEqual([otherUpdate]);
  });
});

describe("linked work item activity identity", () => {
  const pull = {
    kind: "pr" as const,
    repo: "acme/app",
    number: 42,
    updatedAt: 200,
  };
  const issue = {
    kind: "issue" as const,
    repo: "acme/app",
    number: 8,
    updatedAt: 200,
  };

  it("includes the item identity alongside the timestamp", () => {
    expect(linkedWorkItemActivityKey(pull, pull.updatedAt)).not.toBe(
      linkedWorkItemActivityKey(issue, issue.updatedAt),
    );
    expect(sameLinkedWorkItemActivity(pull, issue)).toBe(false);
    expect(sameLinkedWorkItemActivity(pull, pull)).toBe(true);
    expect(sameLinkedWorkItemActivity(undefined, pull)).toBe(false);
  });
});

// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { InboxItem } from "../model/githubTasks";
import { inboxItemKey } from "../model/githubTasks";
import {
  isInboxEntryUnseen,
  markInboxItemSeen,
  seedInboxSeenIfNeeded,
} from "../model/inboxSeen";
import { updateNotificationPreferences } from "../../notifications/model/notificationPreferences";
import type { SessionSummary } from "../../sessions/data/sessionStore";
import { markLinkedSessionUpdateSeen } from "../model/linkedSessionSeen";
import {
  clearPendingInboxSelfActivity,
  recordInboxSelfActivity,
} from "../model/inboxSelfActivity";
import { useInboxActivity, type InboxActivity } from "./useInboxUnseen";

const { githubWorkItem, listInboxItems, playCue } = vi.hoisted(() => ({
  githubWorkItem: vi.fn(),
  listInboxItems: vi.fn(),
  playCue: vi.fn(),
}));
vi.mock("../model/githubTasks", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../model/githubTasks")>()),
  githubWorkItem,
  listInboxItems,
}));
vi.mock("../../settings/model/sounds", () => ({ playCue }));

const remote: InboxItem = {
  provider: "github",
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
  projectPath: "/tmp/app",
};
const session: SessionSummary = {
  id: "linked-session",
  cwd: "/tmp/app",
  harness: "codex",
  model: "gpt-5",
  runtimeMode: "supervised",
  title: "codex · Update sidebar activity",
  createdAt: Date.parse("2026-09-13T10:00:00Z"),
  updatedAt: Date.parse("2026-09-13T10:00:00Z"),
  linkedWorkItem: {
    kind: "pr",
    repo: "acme/app",
    number: 42,
    url: remote.url,
  },
};

let root: Root;
let container: HTMLDivElement;
let activity: InboxActivity;
const recents = [];
const sessions = [session];

function Harness({ rows = sessions }: { rows?: SessionSummary[] }) {
  activity = useInboxActivity(recents, "/tmp/app", rows);
  return null;
}

async function mount(rows = sessions) {
  await act(async () => {
    root.render(createElement(Harness, { rows }));
  });
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  listInboxItems.mockReset();
  githubWorkItem.mockReset();
  playCue.mockReset();
  clearPendingInboxSelfActivity();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  delete (document as { hidden?: boolean }).hidden;
  act(() => root.unmount());
  container.remove();
  localStorage.clear();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  clearPendingInboxSelfActivity();
});

describe("Inbox activity polling", () => {
  it("updates Inbox and linked-session indicators on category changes without consuming unread activity", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-09-14T12:00:00Z"));
    const entry = { key: inboxItemKey(remote), updatedAt: remote.updatedAt };
    seedInboxSeenIfNeeded([{ ...entry, updatedAt: "2026-09-12T12:00:00Z" }]);
    listInboxItems.mockResolvedValue({ items: [remote], errors: {} });
    await mount();
    expect(activity.unseen).toBe(true);
    expect(activity.linkedSessionUpdateIds.has(session.id)).toBe(true);

    act(() =>
      updateNotificationPreferences(["local:/tmp/app"], {
        disabled: ["pullRequests"],
      }),
    );
    expect(activity.unseen).toBe(false);
    expect(activity.linkedSessionUpdateIds.has(session.id)).toBe(false);
    expect(activity.linkedSessionUpdates.has(session.id)).toBe(true);
    expect(isInboxEntryUnseen(entry)).toBe(true);

    act(() =>
      updateNotificationPreferences(["local:/tmp/app"], {
        disabled: [],
        mutedUntil: Date.now() + 1000,
      }),
    );
    expect(activity.unseen).toBe(false);
    expect(activity.linkedSessionUpdateIds.has(session.id)).toBe(false);
    await act(async () => vi.advanceTimersByTimeAsync(1000));
    expect(activity.unseen).toBe(true);
    expect(activity.linkedSessionUpdateIds.has(session.id)).toBe(true);
    expect(isInboxEntryUnseen(entry)).toBe(true);
  });

  it("updates the dot immediately on mute, resume, and mute expiry", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-09-14T12:00:00Z"));
    const entry = { key: inboxItemKey(remote), updatedAt: remote.updatedAt };
    seedInboxSeenIfNeeded([{ ...entry, updatedAt: "2026-09-12T12:00:00Z" }]);
    listInboxItems.mockResolvedValue({ items: [remote], errors: {} });
    await mount();
    expect(activity.unseen).toBe(true);

    act(() =>
      updateNotificationPreferences(["local:/tmp/app"], {
        mutedUntil: null,
      }),
    );
    expect(activity.unseen).toBe(false);
    expect(isInboxEntryUnseen(entry)).toBe(true);
    act(() =>
      updateNotificationPreferences(["local:/tmp/app"], {
        mutedUntil: undefined,
      }),
    );
    expect(activity.unseen).toBe(true);
    act(() =>
      updateNotificationPreferences(["local:/tmp/app"], {
        mutedUntil: Date.now() + 1000,
      }),
    );
    expect(activity.unseen).toBe(false);
    await act(async () => vi.advanceTimersByTimeAsync(1000));
    expect(activity.unseen).toBe(true);
    act(() => markInboxItemSeen(entry));
    expect(activity.unseen).toBe(false);
  });

  it("badges only unmuted projects while muted activity stays unread", async () => {
    const other: InboxItem = {
      ...remote,
      repo: "acme/other",
      url: "https://github.com/acme/other/pull/42",
      projectPath: "/tmp/other",
    };
    const mutedEntry = {
      key: inboxItemKey(remote),
      updatedAt: remote.updatedAt,
    };
    const otherEntry = { key: inboxItemKey(other), updatedAt: other.updatedAt };
    seedInboxSeenIfNeeded([
      { ...mutedEntry, updatedAt: "2026-09-12T12:00:00Z" },
      { ...otherEntry, updatedAt: "2026-09-12T12:00:00Z" },
    ]);
    updateNotificationPreferences(["local:/tmp/app"], {
      mutedUntil: null,
    });
    listInboxItems.mockResolvedValue({ items: [remote, other], errors: {} });
    await mount();

    expect(activity.unseen).toBe(true);
    act(() => markInboxItemSeen(otherEntry));
    expect(activity.unseen).toBe(false);
    expect(isInboxEntryUnseen(mutedEntry)).toBe(true);
    expect(activity.linkedSessionUpdateIds.has(session.id)).toBe(false);
    expect(activity.linkedSessionUpdates.has(session.id)).toBe(true);
  });

  it("reuses the Inbox list for linked-session updates", async () => {
    listInboxItems.mockResolvedValue({ items: [remote], errors: {} });
    await mount();

    expect(activity.linkedSessionUpdateIds.has(session.id)).toBe(true);
    expect(listInboxItems).toHaveBeenCalledTimes(1);
    expect(githubWorkItem).not.toHaveBeenCalled();
  });

  it("does not individually poll linked items omitted by the Inbox", async () => {
    listInboxItems.mockResolvedValue({ items: [], errors: {} });
    githubWorkItem.mockResolvedValue(remote);
    await mount();

    expect(activity.linkedSessionUpdateIds.has(session.id)).toBe(false);
    expect(listInboxItems).toHaveBeenCalledTimes(1);
    expect(githubWorkItem).not.toHaveBeenCalled();
  });

  it("bounds an hour of background traffic regardless of linked-session history", async () => {
    vi.useFakeTimers();
    listInboxItems.mockResolvedValue({ items: [], errors: {} });
    const history = Array.from({ length: 200 }, (_, index) => ({
      ...session,
      id: `old-${index}`,
      linkedWorkItem: { ...session.linkedWorkItem!, number: index + 1000 },
    }));
    await mount(history);
    await act(async () => vi.advanceTimersByTimeAsync(60 * 60_000));

    expect(listInboxItems).toHaveBeenCalledTimes(31);
    expect(githubWorkItem).not.toHaveBeenCalled();
  });

  it("slows tray polling and coalesces repeated visibility changes", async () => {
    vi.useFakeTimers();
    listInboxItems.mockResolvedValue({ items: [remote], errors: {} });
    await mount();
    await act(async () => vi.advanceTimersByTimeAsync(119_999));
    expect(listInboxItems).toHaveBeenCalledTimes(1);
    await act(async () => vi.advanceTimersByTimeAsync(1));
    expect(listInboxItems).toHaveBeenCalledTimes(2);

    Object.defineProperty(document, "hidden", {
      configurable: true,
      value: true,
    });
    await act(async () => vi.advanceTimersByTimeAsync(299_999));
    expect(listInboxItems).toHaveBeenCalledTimes(2);
    await act(async () => vi.advanceTimersByTimeAsync(1));
    expect(listInboxItems).toHaveBeenCalledTimes(3);

    Object.defineProperty(document, "hidden", {
      configurable: true,
      value: false,
    });
    await act(async () => {
      for (let i = 0; i < 10; i++) {
        document.dispatchEvent(new Event("visibilitychange"));
      }
    });
    expect(listInboxItems).toHaveBeenCalledTimes(3);
  });

  it("links a newly loaded session using the existing snapshot without refetching", async () => {
    listInboxItems.mockResolvedValue({ items: [remote], errors: {} });
    await mount([]);
    await mount([session]);
    expect(activity.linkedSessionUpdateIds.has(session.id)).toBe(true);
    expect(listInboxItems).toHaveBeenCalledTimes(1);
    expect(githubWorkItem).not.toHaveBeenCalled();
  });

  it("clears a linked-session update as soon as its remote snapshot is read", async () => {
    listInboxItems.mockResolvedValue({ items: [remote], errors: {} });
    await mount();
    expect(activity.linkedSessionUpdateIds.has(session.id)).toBe(true);

    act(() => {
      markLinkedSessionUpdateSeen(session.id, Date.parse(remote.updatedAt));
    });

    expect(activity.linkedSessionUpdateIds.has(session.id)).toBe(false);
  });

  it("acknowledges an app-authored revision without a cue or linked-session notification", async () => {
    let listed = { ...remote, updatedAt: "2026-09-13T11:00:00Z" };
    listInboxItems.mockImplementation(async () => ({
      items: [listed],
      errors: {},
    }));
    markLinkedSessionUpdateSeen(session.id, Date.parse(listed.updatedAt));
    await mount();
    expect(activity.unseen).toBe(false);
    expect(activity.linkedSessionUpdateIds.has(session.id)).toBe(false);

    listed = { ...listed, updatedAt: "2026-09-13T12:05:00Z" };
    await act(async () => {
      recordInboxSelfActivity({
        provider: "github",
        kind: "pr",
        repo: listed.repo,
        number: listed.number,
      });
      await Promise.resolve();
      await Promise.resolve();
    });

    const entry = { key: inboxItemKey(listed), updatedAt: listed.updatedAt };
    expect(listInboxItems).toHaveBeenCalledTimes(2);
    expect(playCue).not.toHaveBeenCalled();
    expect(isInboxEntryUnseen(entry)).toBe(false);
    expect(activity.unseen).toBe(false);
    expect(activity.linkedSessionUpdateIds.has(session.id)).toBe(false);
  });
});

describe("Inbox activity for automations", () => {
  const POLL_MS = 2 * 60_000;
  const onActivity = vi.fn();

  function AutomationHarness({
    projects = recents,
  }: {
    projects?: typeof recents;
  }) {
    useInboxActivity(projects, "/tmp/app", [], { onActivity });
    return null;
  }

  async function mountForAutomations() {
    await act(async () => {
      root.render(createElement(AutomationHarness));
    });
  }

  async function nextPoll() {
    await act(async () => vi.advanceTimersByTimeAsync(POLL_MS));
  }

  const reportedTransitions = () =>
    onActivity.mock.calls.flatMap(([, transitions]) => transitions);
  const reportedAppeared = () =>
    onActivity.mock.calls.flatMap(([appeared]) => appeared);

  beforeEach(() => {
    onActivity.mockReset();
    vi.useFakeTimers();
  });

  it("reports a pull request that left the open list because it was merged", async () => {
    const merged = {
      ...remote,
      state: "merged",
      updatedAt: "2026-09-13T13:00:00Z",
    };
    listInboxItems
      .mockResolvedValueOnce({ items: [remote], errors: {} })
      .mockResolvedValue({ items: [], errors: {} });
    githubWorkItem.mockResolvedValue(merged);
    await mountForAutomations();
    expect(reportedTransitions()).toEqual([]);

    await nextPoll();
    expect(githubWorkItem).toHaveBeenCalledExactlyOnceWith(
      "/tmp/app",
      "acme/app",
      "pr",
      42,
      { force: true },
    );
    expect(reportedTransitions()).toEqual([
      { item: merged, transition: "merged" },
    ]);

    await nextPoll();
    expect(reportedTransitions()).toHaveLength(1);
    expect(githubWorkItem).toHaveBeenCalledTimes(1);
  });

  it("reports nothing when the missing pull request is still open", async () => {
    listInboxItems
      .mockResolvedValueOnce({ items: [remote], errors: {} })
      .mockResolvedValue({ items: [], errors: {} });
    githubWorkItem.mockResolvedValue(remote);
    await mountForAutomations();
    await nextPoll();
    await nextPoll();

    expect(githubWorkItem).toHaveBeenCalledTimes(1);
    expect(reportedTransitions()).toEqual([]);
  });

  it("retries a lookup cancelled by an effect replacement", async () => {
    const merged = { ...remote, state: "merged" };
    let finishLookup!: (item: InboxItem) => void;
    let finishList!: (result: { items: InboxItem[]; errors: {} }) => void;
    listInboxItems
      .mockResolvedValueOnce({ items: [remote], errors: {} })
      .mockResolvedValueOnce({ items: [], errors: {} })
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finishList = resolve;
          }),
      );
    githubWorkItem
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finishLookup = resolve;
          }),
      )
      .mockResolvedValue(merged);
    await mountForAutomations();
    await nextPoll();
    expect(githubWorkItem).toHaveBeenCalledTimes(1);

    await act(async () => {
      root.render(createElement(AutomationHarness, { projects: [...recents] }));
    });
    await act(async () => {
      finishLookup(merged);
    });
    expect(reportedTransitions()).toEqual([]);
    await act(async () => {
      finishList({ items: [], errors: {} });
    });
    expect(reportedTransitions()).toEqual([
      { item: merged, transition: "merged" },
    ]);
    expect(githubWorkItem).toHaveBeenCalledTimes(2);
  });

  it("hands off earlier confirmed lookups when a later lookup is cancelled", async () => {
    const other = { ...remote, number: 43 };
    const merged = { ...remote, state: "merged" };
    let finishLookup!: (item: InboxItem) => void;
    let finishList!: (result: { items: InboxItem[]; errors: {} }) => void;
    listInboxItems
      .mockResolvedValueOnce({ items: [remote, other], errors: {} })
      .mockResolvedValueOnce({ items: [], errors: {} })
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finishList = resolve;
          }),
      );
    githubWorkItem
      .mockResolvedValueOnce(merged)
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finishLookup = resolve;
          }),
      )
      .mockResolvedValue({ ...other, state: "open" });
    await mountForAutomations();
    await nextPoll();
    expect(githubWorkItem).toHaveBeenCalledTimes(2);
    await act(async () => {
      root.render(createElement(AutomationHarness, { projects: [...recents] }));
    });
    await act(async () => {
      finishLookup({ ...other, state: "merged" });
    });
    expect(reportedTransitions()).toEqual([
      { item: merged, transition: "merged" },
    ]);
    await act(async () => {
      finishList({ items: [], errors: {} });
    });
    expect(reportedTransitions()).toEqual([
      { item: merged, transition: "merged" },
    ]);
  });

  it("reports a reopened item without treating it as newly appeared", async () => {
    const closed = { ...remote, state: "closed" };
    const reopened = { ...remote, updatedAt: "2026-09-13T14:00:00Z" };
    listInboxItems
      .mockResolvedValueOnce({ items: [closed], errors: {} })
      .mockResolvedValue({ items: [reopened], errors: {} });
    await mountForAutomations();
    await nextPoll();

    expect(reportedTransitions()).toEqual([
      { item: reopened, transition: "reopened" },
    ]);
    expect(reportedAppeared()).toEqual([]);
    expect(githubWorkItem).not.toHaveBeenCalled();
  });

  it("does not look items up while the GitHub refresh is failing", async () => {
    listInboxItems
      .mockResolvedValueOnce({ items: [remote], errors: {} })
      .mockResolvedValue({ items: [], errors: { github: "gh is offline" } });
    await mountForAutomations();
    await nextPoll();

    expect(githubWorkItem).not.toHaveBeenCalled();
    expect(reportedTransitions()).toEqual([]);
  });

  it("reports head changes through the shared poll and catches up after remount without extra fetches", async () => {
    const a = { ...remote, headRefOid: "a".repeat(40) };
    const b = { ...remote, headRefOid: "b".repeat(40) };
    listInboxItems.mockResolvedValue({ items: [a], errors: {} });
    await mountForAutomations();
    expect(reportedTransitions()).toEqual([]);
    expect(localStorage.getItem("monocode.inbox-pr-heads.v1")).toContain(
      a.headRefOid,
    );
    await act(async () => {
      root.render(null);
    });
    listInboxItems.mockResolvedValue({ items: [b], errors: {} });
    await mountForAutomations();
    expect(reportedTransitions()).toEqual([
      { item: b, transition: "head_changed", previousHead: a.headRefOid },
    ]);
    expect(onActivity.mock.calls.at(-1)?.[2]).toEqual([b]);
    await nextPoll();
    expect(reportedTransitions()).toHaveLength(1);
    expect(githubWorkItem).not.toHaveBeenCalled();
    listInboxItems.mockResolvedValue({
      items: [b],
      errors: { github: "offline" },
    });
    await nextPoll();
    expect(onActivity.mock.calls.at(-1)?.[2]).toEqual([]);
  });

  it("still reports newly appeared items on every poll", async () => {
    const other = { ...remote, number: 43 };
    listInboxItems
      .mockResolvedValueOnce({ items: [remote], errors: {} })
      .mockResolvedValue({ items: [remote, other], errors: {} });
    await mountForAutomations();
    await nextPoll();

    expect(reportedAppeared()).toEqual([other]);
    expect(onActivity.mock.calls.length).toBeGreaterThanOrEqual(2);
  });
});

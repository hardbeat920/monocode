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

function Harness() {
  activity = useInboxActivity(recents, "/tmp/app", sessions);
  return null;
}

async function mount() {
  await act(async () => {
    root.render(createElement(Harness));
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

  it("falls back to an exact lookup only when the Inbox omits the item", async () => {
    listInboxItems.mockResolvedValue({ items: [], errors: {} });
    githubWorkItem.mockResolvedValue(remote);
    await mount();

    expect(activity.linkedSessionUpdateIds.has(session.id)).toBe(true);
    expect(listInboxItems).toHaveBeenCalledTimes(1);
    expect(githubWorkItem).toHaveBeenCalledExactlyOnceWith(
      "/tmp/app",
      "acme/app",
      "pr",
      42,
      { force: true },
    );
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

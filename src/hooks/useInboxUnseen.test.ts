// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { InboxItem } from "../lib/githubTasks";
import type { SessionSummary } from "../lib/sessionStore";
import { markLinkedSessionUpdateSeen } from "../lib/linkedSessionSeen";
import { useInboxActivity, type InboxActivity } from "./useInboxUnseen";

const { githubWorkItem, listInboxItems } = vi.hoisted(() => ({
  githubWorkItem: vi.fn(),
  listInboxItems: vi.fn(),
}));
vi.mock("../lib/githubTasks", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/githubTasks")>()),
  githubWorkItem,
  listInboxItems,
}));
vi.mock("../lib/sounds", () => ({ noteInboxUnseen: vi.fn() }));

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
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  localStorage.clear();
  vi.unstubAllGlobals();
});

describe("Inbox activity polling", () => {
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
});

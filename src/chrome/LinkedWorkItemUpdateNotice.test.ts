// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LinkedWorkItemUpdateCard } from "../lib/linkedWorkItemActivity";
import { LinkedWorkItemUpdateNotice } from "./LinkedWorkItemUpdateNotice";

const { openUrl, playCue } = vi.hoisted(() => ({
  openUrl: vi.fn(),
  playCue: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }));
vi.mock("../lib/sounds", () => ({ playCue }));

const baseCard: LinkedWorkItemUpdateCard = {
  kind: "pr",
  repo: "acme/app",
  number: 42,
  title: "Update sidebar activity",
  url: "https://github.com/acme/app/pull/42",
  state: "open",
  since: Date.parse("2026-09-13T10:00:00Z"),
  updatedAt: Date.parse("2026-09-13T12:00:00Z"),
  status: "ready",
  counts: { comments: 1, reviews: 0, commits: 0 },
  entries: [
    {
      id: "comment-1",
      kind: "comment",
      author: "maya",
      text: "Please cover the empty state",
      createdAt: "2026-09-13T11:00:00Z",
      url: "https://github.com/acme/app/pull/42#issuecomment-1",
    },
  ],
  truncated: false,
};

let root: Root;
let container: HTMLDivElement;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  openUrl.mockReset();
  playCue.mockReset();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

function button(label: string): HTMLButtonElement {
  const match = [...document.body.querySelectorAll("button")].find(
    (entry) => entry.textContent?.trim() === label,
  );
  if (!(match instanceof HTMLButtonElement)) {
    throw new Error(`Missing button: ${label}`);
  }
  return match;
}

describe("linked work item update notice", () => {
  it("stays hidden while details load and animates in when ready", () => {
    act(() => {
      root.render(
        createElement(LinkedWorkItemUpdateNotice, {
          card: { ...baseCard, status: "loading" },
          onAcknowledge: () => {},
          onDismiss: () => {},
          onOpenDiscussion: () => {},
          onAddToChat: () => {},
        }),
      );
    });
    expect(
      document.body.querySelector('[aria-label^="New activity on"]'),
    ).toBeNull();

    act(() => {
      root.render(
        createElement(LinkedWorkItemUpdateNotice, {
          card: baseCard,
          onAcknowledge: () => {},
          onDismiss: () => {},
          onOpenDiscussion: () => {},
          onAddToChat: () => {},
        }),
      );
    });
    expect(
      document.body.querySelector('[aria-label^="New activity on"]')?.classList,
    ).toContain("linked-activity-notice");
    expect(playCue).toHaveBeenCalledExactlyOnceWith("linkedActivity");

    act(() => {
      root.render(
        createElement(LinkedWorkItemUpdateNotice, {
          onAcknowledge: () => {},
          onDismiss: () => {},
          onOpenDiscussion: () => {},
          onAddToChat: () => {},
        }),
      );
    });
    act(() => {
      root.render(
        createElement(LinkedWorkItemUpdateNotice, {
          card: baseCard,
          onAcknowledge: () => {},
          onDismiss: () => {},
          onOpenDiscussion: () => {},
          onAddToChat: () => {},
        }),
      );
    });
    expect(playCue).toHaveBeenCalledTimes(1);
  });

  it("offers discussion and agent actions for a new comment", () => {
    const onAcknowledge = vi.fn();
    const onOpenDiscussion = vi.fn();
    const onAddToChat = vi.fn();
    act(() => {
      root.render(
        createElement(LinkedWorkItemUpdateNotice, {
          card: baseCard,
          onAcknowledge,
          onDismiss: () => {},
          onOpenDiscussion,
          onAddToChat,
        }),
      );
    });

    const notice = document.body.querySelector<HTMLElement>(
      'section[aria-label="New activity on Pull request 42"]',
    );
    expect(notice?.classList.contains("isolate")).toBe(true);
    expect(notice?.classList.contains("bg-content/10")).toBe(false);
    expect(notice?.classList.contains("bg-background-base/95")).toBe(false);
    expect(notice?.querySelector(".popover-backdrop")).not.toBeNull();
    expect(document.body.textContent).toContain("1 new comment");
    expect(
      button("Dismiss").nextElementSibling?.getAttribute("aria-label"),
    ).toBe("Dismiss updates for Pull request 42");
    expect(button("Open discussion").classList.contains("truncate")).toBe(true);
    expect(button("Address with agent").classList).toContain(
      "whitespace-nowrap",
    );
    act(() => button("Open discussion").click());
    expect(onAcknowledge).toHaveBeenCalledTimes(1);
    expect(onOpenDiscussion).toHaveBeenCalledTimes(1);
    act(() => button("Address with agent").click());
    expect(onAcknowledge).toHaveBeenCalledTimes(2);
    expect(onAddToChat).toHaveBeenCalledWith(
      expect.stringContaining("Please cover the empty state"),
    );
  });

  it("opens the exact commit for commit activity", () => {
    const onAcknowledge = vi.fn();
    const card: LinkedWorkItemUpdateCard = {
      ...baseCard,
      counts: { comments: 0, reviews: 0, commits: 1 },
      entries: [
        {
          id: "abcdef123456",
          kind: "commit",
          author: "nik",
          text: "Handle linked activity",
          createdAt: "2026-09-13T11:30:00Z",
          url: "https://github.com/acme/app/commit/abcdef123456",
        },
      ],
    };
    act(() => {
      root.render(
        createElement(LinkedWorkItemUpdateNotice, {
          card,
          onAcknowledge,
          onDismiss: () => {},
          onOpenDiscussion: () => {},
          onAddToChat: () => {},
        }),
      );
    });

    act(() => button("Open commit").click());
    expect(onAcknowledge).toHaveBeenCalledTimes(1);
    expect(openUrl).toHaveBeenCalledWith(
      "https://github.com/acme/app/commit/abcdef123456",
    );
  });

  it("offers archive and confirmed-delete flows for a merged pull request", async () => {
    const onAcknowledge = vi.fn();
    const onArchiveSession = vi.fn().mockResolvedValue(true);
    const onDeleteSession = vi.fn().mockResolvedValue(false);
    act(() => {
      root.render(
        createElement(LinkedWorkItemUpdateNotice, {
          card: { ...baseCard, state: "merged" },
          onAcknowledge,
          onDismiss: () => {},
          onOpenDiscussion: () => {},
          onAddToChat: () => {},
          onArchiveSession,
          onDeleteSession,
        }),
      );
    });

    expect(document.body.textContent).toContain("Pull request merged");
    expect(
      button("Archive session").querySelector("span")?.classList,
    ).toContain("truncate");
    await act(async () => button("Delete…").click());
    expect(onDeleteSession).toHaveBeenCalledTimes(1);
    expect(onAcknowledge).not.toHaveBeenCalled();

    await act(async () => button("Archive session").click());
    expect(onArchiveSession).toHaveBeenCalledTimes(1);
    expect(onAcknowledge).toHaveBeenCalledTimes(1);
  });

  it("offers session cleanup when a linked issue closes", () => {
    act(() => {
      root.render(
        createElement(LinkedWorkItemUpdateNotice, {
          card: { ...baseCard, kind: "issue", state: "closed" },
          onAcknowledge: () => {},
          onDismiss: () => {},
          onOpenDiscussion: () => {},
          onAddToChat: () => {},
          onArchiveSession: async () => true,
          onDeleteSession: async () => true,
        }),
      );
    });

    expect(document.body.textContent).toContain("Issue closed");
    expect(button("Archive session")).toBeTruthy();
    expect(button("Delete…")).toBeTruthy();
  });
});

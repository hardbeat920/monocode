// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(async () => undefined),
}));

vi.mock("../lib/githubTasks", () => ({
  formatRelativeTime: () => "2 hours ago",
  githubWorkItem: vi.fn(async () => ({
    kind: "pr",
    number: 73,
    title: "Make work item links easier to scan",
    url: "https://github.com/acme/widgets/pull/73",
    state: "open",
    createdAt: "2026-09-18T10:00:00Z",
    updatedAt: "2026-09-19T10:00:00Z",
    labels: [{ name: "enhancement", color: "8b5cf6" }],
    assignees: [{ login: "grace", avatarUrl: "" }],
    draft: false,
    repo: "acme/widgets",
  })),
  githubWorkItemDetails: vi.fn(async () => ({
    body: "Adds **compact chips** and a useful hover preview.",
    author: "ada",
    authorAvatarUrl: "",
    baseRefName: "main",
    headRefName: "link-chips",
    reviewDecision: "",
  })),
  peekGithubWorkItem: () => null,
  peekGithubWorkItemDetails: () => null,
}));

import { openUrl } from "@tauri-apps/plugin-opener";
import { githubWorkItem, githubWorkItemDetails } from "../lib/githubTasks";
import type { UserLink } from "../lib/linkPreview";
import { UserLinkPreview } from "./UserLinkPreview";

const link: UserLink = {
  url: "https://github.com/acme/widgets/pull/73",
  host: "github.com",
  displayUrl: "github.com/acme/widgets/pull/73",
  githubWorkItem: { kind: "pr", repo: "acme/widgets", number: 73 },
};

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

describe("GitHub work item link preview", () => {
  it("loads a rich keyboard-accessible popover when the chip receives focus", async () => {
    act(() =>
      root.render(
        createElement(UserLinkPreview, { link, cwd: "/workspace/widgets" }),
      ),
    );

    const chip = container.querySelector<HTMLAnchorElement>(
      '[data-github-work-item-chip="pr"]',
    )!;
    expect(chip.textContent).toContain("PR");
    expect(chip.textContent).toContain("#73");
    expect(chip.textContent).toBe("PR #73");

    await act(async () => {
      chip.focus();
      await Promise.resolve();
      await Promise.resolve();
    });

    const popover = document.querySelector<HTMLElement>(
      "[data-github-work-item-popover]",
    )!;
    expect(popover.getAttribute("role")).toBe("tooltip");
    expect(popover.textContent).toContain(
      "Make work item links easier to scan",
    );
    expect(popover.textContent).toContain(
      "Adds compact chips and a useful hover preview.",
    );
    expect(popover.textContent).toContain("ada");
    expect(popover.textContent).toContain("main ← link-chips");
    expect(popover.textContent).toContain("enhancement");
    expect(popover.textContent).toContain("Updated 2 hours ago");
    expect(chip.getAttribute("aria-describedby")).toBe(popover.id);
    expect(githubWorkItem).toHaveBeenCalledWith(
      "/workspace/widgets",
      "acme/widgets",
      "pr",
      73,
    );
    expect(githubWorkItemDetails).toHaveBeenCalledWith(
      "/workspace/widgets",
      "acme/widgets",
      "pr",
      73,
    );
  });

  it("opens after a short hover delay", async () => {
    vi.useFakeTimers();
    act(() => root.render(createElement(UserLinkPreview, { link })));
    const chip = container.querySelector<HTMLAnchorElement>(
      '[data-github-work-item-chip="pr"]',
    )!;

    act(() =>
      chip.dispatchEvent(new MouseEvent("mouseover", { bubbles: true })),
    );
    act(() => vi.advanceTimersByTime(219));
    expect(
      document.querySelector("[data-github-work-item-popover]"),
    ).toBeNull();

    await act(async () => {
      vi.advanceTimersByTime(1);
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(
      document.querySelector("[data-github-work-item-popover]"),
    ).not.toBeNull();
  });

  it("keeps the chip clickable and opens the original URL", () => {
    act(() => root.render(createElement(UserLinkPreview, { link })));
    const chip = container.querySelector<HTMLAnchorElement>(
      '[data-github-work-item-chip="pr"]',
    )!;

    act(() => chip.click());

    expect(openUrl).toHaveBeenCalledWith(link.url);
  });

  it("uses a compact treatment centered with inline message text", () => {
    act(() =>
      root.render(createElement(UserLinkPreview, { link, compact: true })),
    );
    const chip = container.querySelector<HTMLAnchorElement>(
      '[data-github-work-item-chip="pr"]',
    )!;

    expect(chip.getAttribute("data-compact")).toBe("true");
    expect(chip.className).toContain("text-xs");
    expect(chip.className).toContain("align-middle");
    expect(chip.className).toContain("border-0");
    expect(chip.className).toContain("p-0");
    expect(chip.className).toContain("gap-1");
    expect(chip.querySelector("span")?.className).toContain("h-[18px]");
  });
});

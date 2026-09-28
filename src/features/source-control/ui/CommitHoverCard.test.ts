// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../platform/tauri/fs", () => ({
  gitCommitFiles: vi.fn(),
  gitCommitMessage: vi.fn(),
}));
vi.mock("../../../platform/tauri/clipboard", () => ({
  copyText: vi.fn(async () => undefined),
}));

import { copyText } from "../../../platform/tauri/clipboard";
import {
  gitCommitFiles,
  gitCommitMessage,
  type GitChangedFile,
  type GitHistoryCommit,
} from "../../../platform/tauri/fs";
import type { GraphRef } from "../model/gitGraph";
import { CommitHoverCard, REVEAL_BUDGET_MS } from "./CommitHoverCard";

const mockFiles = vi.mocked(gitCommitFiles);
const mockMessage = vi.mocked(gitCommitMessage);
const mockCopy = vi.mocked(copyText);

function file(additions: number, deletions: number): GitChangedFile {
  return {
    path: "/workspace/web/a.ts",
    relative: "a.ts",
    status: "modified",
    additions,
    deletions,
    staged: false,
    unstaged: false,
  };
}

const commit: GitHistoryCommit = {
  sha: "abcdef1234567890abcdef1234567890abcdef12",
  shortSha: "abcdef1",
  parents: ["0000000000000000000000000000000000000000"],
  author: "Grace Hopper",
  timestamp: Math.floor(Date.parse("2026-09-28T13:45:00Z") / 1000),
  subject: "Tighten history row density",
  refs: [
    { name: "feature/a-very-long-branch-name", kind: "local" },
    { name: "origin/feature/a-very-long-branch-name", kind: "remote" },
  ],
  head: true,
};

const refs: GraphRef[] = [
  { name: "feature/a-very-long-branch-name", kind: "local", color: "#FFB000" },
  { name: "origin/feature/a-very-long-branch-name", kind: "remote" },
];

let container: HTMLDivElement;
let anchor: HTMLButtonElement;
let root: Root;

function render(cwd: string, overrides: Record<string, unknown> = {}) {
  act(() =>
    root.render(
      createElement(CommitHoverCard, {
        cwd,
        commit,
        refs,
        anchor,
        id: "commit-tooltip",
        onDismiss: () => undefined,
        onFocusLeave: () => undefined,
        onReturnFocus: () => undefined,
        onTabForward: () => false,
        onPointerEnter: () => undefined,
        onPointerLeave: () => undefined,
        ...overrides,
      }),
    ),
  );
}

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

async function click(selector: string) {
  const button = tooltip().querySelector<HTMLButtonElement>(selector);
  expect(button, `no button matched ${selector}`).not.toBeNull();
  await act(async () => {
    button!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

function tooltip(): HTMLElement {
  return document.querySelector<HTMLElement>('[role="dialog"]')!;
}

/** The card is withheld until its message settles, so absence is a real state. */
function tooltipOrNull(): HTMLElement | null {
  return document.querySelector<HTMLElement>('[role="dialog"]');
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  mockFiles.mockReset();
  mockFiles.mockResolvedValue([file(1, 1)]);
  mockMessage.mockReset();
  mockMessage.mockResolvedValue(commit.subject);
  mockCopy.mockClear();
  container = document.createElement("div");
  anchor = document.createElement("button");
  container.append(anchor);
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("CommitHoverCard reveal", () => {
  it("waits for the message so the card never paints without its body", async () => {
    let resolveMessage!: (text: string) => void;
    mockMessage.mockReturnValue(
      new Promise<string>((r) => {
        resolveMessage = r;
      }),
    );
    render("/workspace/gate");
    await flush();

    // Nothing on screen yet: the animation must not play on a bodyless card
    // that then grows by hundreds of pixels under the reader.
    expect(tooltipOrNull()).toBeNull();

    await act(async () => {
      resolveMessage(`${commit.subject}\n\n- a detail line`);
      await flush();
    });
    expect(tooltipOrNull()).not.toBeNull();
    expect(tooltipOrNull()!.textContent).toContain("- a detail line");
  });

  it("shows the card anyway once the reveal budget expires", async () => {
    vi.useFakeTimers();
    // Git never answers, so only the budget can unblock the card.
    mockMessage.mockReturnValue(new Promise<string>(() => undefined));
    render("/workspace/gate-timeout");
    await act(async () => {
      await Promise.resolve();
    });
    expect(tooltipOrNull()).toBeNull();

    await act(async () => {
      vi.advanceTimersByTime(REVEAL_BUDGET_MS);
    });
    expect(tooltipOrNull()).not.toBeNull();
    // A body-less card is the right thing to show here, not a spinner.
    expect(tooltipOrNull()!.querySelector("div.max-h-60")).toBeNull();
  });

  it("shows the card straight away with no project folder", async () => {
    vi.useFakeTimers();
    render("~");
    await act(async () => {
      await Promise.resolve();
    });
    // There is no Git to ask, so the budget must not delay anything.
    expect(tooltipOrNull()).not.toBeNull();
    expect(mockMessage).not.toHaveBeenCalled();
  });

  it("does not keep the card's body height moving when stats land", async () => {
    let resolveFiles!: (files: GitChangedFile[]) => void;
    mockFiles.mockReturnValue(
      new Promise<GitChangedFile[]>((r) => {
        resolveFiles = r;
      }),
    );
    render("/workspace/stats-height");
    await flush();

    // `motion-safe:animate-pulse` is one class token, so match the substring.
    const placeholder = tooltipOrNull()!.querySelector<HTMLElement>(
      '[class*="animate-pulse"]',
    )!;
    // `h-4` is the `leading-4` of the real line, so the swap is pixel-stable.
    expect(placeholder.classList).toContain("h-4");
    // A decorative placeholder must not be announced as a labelled region.
    expect(placeholder.getAttribute("aria-label")).toBeNull();
    expect(placeholder.getAttribute("aria-hidden")).toBe("true");

    await act(async () => {
      resolveFiles([file(1, 1)]);
      await flush();
    });
    expect(tooltipOrNull()!.textContent).toContain("1 file changed");
  });
});

describe("CommitHoverCard", () => {
  it("leads with the author, date, message and changed-file summary", async () => {
    mockFiles.mockResolvedValue([file(1, 2), file(3, 4)]);
    render("/workspace/lead");
    await flush();

    const card = tooltip();
    expect(card.id).toBe("commit-tooltip");
    expect(card.textContent).toContain(commit.author);
    expect(card.textContent).toContain("2026");
    expect(card.textContent).toContain(commit.subject);
    expect(card.textContent).toContain("2 files changed");
    expect(card.textContent).toContain("4 insertions(+)");
    expect(card.textContent).toContain("6 deletions(-)");
    expect(card.textContent).toContain(commit.shortSha);
  });

  it("holds a fixed width so the card does not resize between rows", async () => {
    render("/workspace/width");
    await flush();

    // A card that shrink-wraps changes width on every row in the list — no refs
    // versus three long branch names, "fix" versus a long body — so it flickers
    // as the pointer moves down. The width is a class, not a measurement, which
    // is also why happy-dom can assert it: there is no layout to read.
    const classes = Array.from(tooltip().classList);
    expect(classes).toContain("w-[28rem]");
    expect(classes).toContain("max-w-full");
    expect(classes).not.toContain("w-max");
    // No fixed pixel width is handed to the Popover: a measured width would
    // have to restate the padding and border sizes as a magic number.
    expect(tooltip().getAttribute("style") ?? "").not.toContain("width:");
  });

  it("keeps the same width for a bare commit and a busy one", async () => {
    const bare = { ...commit, author: "", subject: "fix", refs: [] };
    const busy = {
      ...commit,
      author: "A Very Long Author Name Indeed",
      subject: "x".repeat(400),
      refs: [
        { name: "feature/an-extremely-long-branch-name", kind: "local" },
        {
          name: "origin/feature/an-extremely-long-branch-name",
          kind: "remote",
        },
      ],
    };
    const widthClass = (el: Element) =>
      Array.from(el.classList).find((name) => name.startsWith("w-["));

    act(() =>
      root.render(
        createElement(CommitHoverCard, {
          cwd: "/workspace/bare",
          commit: bare,
          refs: [],
          anchor,
          id: "bare-card",
          onDismiss: () => undefined,
          onFocusLeave: () => undefined,
          onReturnFocus: () => undefined,
          onTabForward: () => false,
          onPointerEnter: () => undefined,
          onPointerLeave: () => undefined,
        }),
      ),
    );
    await flush();
    const bareWidth = widthClass(document.getElementById("bare-card")!);

    act(() =>
      root.render(
        createElement(CommitHoverCard, {
          cwd: "/workspace/busy",
          commit: busy,
          refs: busy.refs,
          anchor,
          id: "busy-card",
          onDismiss: () => undefined,
          onFocusLeave: () => undefined,
          onReturnFocus: () => undefined,
          onTabForward: () => false,
          onPointerEnter: () => undefined,
          onPointerLeave: () => undefined,
        }),
      ),
    );
    await flush();

    expect(busy.subject.length).toBeGreaterThan(bare.subject.length);
    expect(widthClass(document.getElementById("busy-card")!)).toBe(bareWidth);
  });

  it("keeps a long ref name discoverable even though the chip truncates", async () => {
    render("/workspace/refs");
    await flush();

    // The card is a fixed width now, so a very long branch name can outgrow
    // its chip. It truncates the way the row's pill does, and the full name
    // stays in the DOM and in the tooltip attribute.
    const chip = tooltip().querySelector<HTMLElement>(
      '[title="feature/a-very-long-branch-name"]',
    )!;
    expect(chip.textContent).toBe("feature/a-very-long-branch-name");
    expect(chip.querySelector(".truncate")).not.toBeNull();
  });

  it("shows the description body and scrolls it past its cap", async () => {
    const body = Array.from(
      { length: 40 },
      (_, index) => `- detail line ${index}`,
    ).join("\n");
    mockMessage.mockResolvedValue(`${commit.subject}\n\n${body}`);
    render("/workspace/body");
    await flush();

    const card = tooltip();
    expect(card.textContent).toContain("- detail line 39");
    expect(card.querySelector("div.max-h-60")).not.toBeNull();
    expect(card.querySelector("div.max-h-60")!.classList).toContain(
      "overflow-y-auto",
    );
  });

  it("omits the description block for a subject-only commit", async () => {
    render("/workspace/no-body");
    await flush();

    expect(tooltip().querySelector("div.max-h-60")).toBeNull();
  });

  it("shows full ref names the row truncates", async () => {
    mockFiles.mockResolvedValue([file(1, 1)]);
    render("/workspace/refs");
    await flush();

    expect(tooltip().textContent).toContain("feature/a-very-long-branch-name");
    expect(tooltip().textContent).toContain(
      "origin/feature/a-very-long-branch-name",
    );
    const chip = tooltip().querySelector<HTMLElement>(
      '[title="feature/a-very-long-branch-name"]',
    )!;
    expect(chip.style.backgroundColor).toBe("#FFB000");
  });

  it("copies the full SHA and the verbatim message", async () => {
    mockFiles.mockResolvedValue([file(1, 1)]);
    const fullMessage = `${commit.subject}\nBody without blank separator\n\nTrailer\n`;
    mockMessage.mockResolvedValue(fullMessage);
    render("/workspace/copy");
    await flush();

    const card = tooltip();
    await click('[aria-label="Copy commit SHA"]');
    expect(mockCopy).toHaveBeenCalledWith(commit.sha);

    // Verbatim, not a re-join of subject and description: the blank line the
    // commit actually has has to survive the round trip.
    await click('[aria-label="Copy commit message"]');
    expect(mockCopy).toHaveBeenCalledWith(fullMessage);
  });

  it("falls back to the row's subject when Git cannot report the message", async () => {
    mockMessage.mockRejectedValue(new Error("git exploded"));
    render("/workspace/copy-fallback");
    await flush();

    await click('[aria-label="Copy commit message"]');
    expect(mockCopy).toHaveBeenCalledWith(commit.subject);
  });

  it("names both copy actions for assistive tech", async () => {
    render("/workspace/labels");
    await flush();

    const labels = Array.from(
      tooltip().querySelectorAll("button[aria-label]"),
      (button) => button.getAttribute("aria-label"),
    );
    expect(labels).toEqual(["Copy commit SHA", "Copy commit message"]);
  });

  it("labels the card for assistive tech", async () => {
    render("/workspace/aria");
    await flush();

    const card = tooltip();
    expect(card.getAttribute("role")).toBe("dialog");
    expect(card.getAttribute("aria-label")).toBe(
      `Commit ${commit.shortSha} details`,
    );
  });

  it("wraps Tab around its own actions instead of leaving the card", async () => {
    const onReturnFocus = vi.fn();
    const onTabForward = vi.fn(() => true);
    render("/workspace/tab", { onReturnFocus, onTabForward });
    await flush();

    const card = tooltip();
    const [first, last] = Array.from(
      card.querySelectorAll<HTMLButtonElement>("button"),
    );

    act(() =>
      first!.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Tab",
          shiftKey: true,
          bubbles: true,
          cancelable: true,
        }),
      ),
    );
    expect(onReturnFocus).toHaveBeenCalledTimes(1);

    act(() =>
      last!.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Tab",
          bubbles: true,
          cancelable: true,
        }),
      ),
    );
    expect(onTabForward).toHaveBeenCalledTimes(1);
  });

  it("leaves Tab alone when the card cannot hand focus onward", async () => {
    const onTabForward = vi.fn(() => false);
    render("/workspace/tab-end", { onTabForward });
    await flush();

    const last = Array.from(
      tooltip().querySelectorAll<HTMLButtonElement>("button"),
    ).pop()!;
    const event = new KeyboardEvent("keydown", {
      key: "Tab",
      bubbles: true,
      cancelable: true,
    });
    act(() => last.dispatchEvent(event));

    expect(onTabForward).toHaveBeenCalledTimes(1);
    expect(event.defaultPrevented).toBe(false);
  });

  it("degrades when Git cannot report the commit", async () => {
    mockFiles.mockRejectedValue(new Error("git exploded"));
    render("/workspace/broken");
    await flush();

    expect(tooltip().textContent).toContain("Changed files unavailable");
  });
});

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
import { CommitHoverCard } from "./CommitHoverCard";

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

function render(cwd: string) {
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
      }),
    ),
  );
}

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

function tooltip(): HTMLElement {
  return document.querySelector<HTMLElement>('[role="dialog"]')!;
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
  vi.unstubAllGlobals();
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

  it("measures the header while keeping card content within the popover width", async () => {
    render("/workspace/width");
    await flush();

    const card = tooltip();
    expect(card.classList).toContain("w-full");
    expect(card.classList).toContain("min-w-0");
    expect(card.querySelector('[aria-hidden="true"].w-max')).not.toBeNull();
    expect(card.style.width).toBe("");
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

  it("copies the full SHA and the message", async () => {
    mockFiles.mockResolvedValue([file(1, 1)]);
    const fullMessage = `${commit.subject}\nBody without blank separator\n\nTrailer\n`;
    mockMessage.mockResolvedValue(fullMessage);
    render("/workspace/copy");
    await flush();

    const card = tooltip();
    await act(async () => {
      card
        .querySelector<HTMLButtonElement>('[aria-label="Copy commit SHA"]')!
        .dispatchEvent(new MouseEvent("click", { bubbles: true }));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(mockCopy).toHaveBeenCalledWith(commit.sha);

    await act(async () => {
      card
        .querySelector<HTMLButtonElement>("button:not([aria-label])")!
        .dispatchEvent(new MouseEvent("click", { bubbles: true }));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(mockCopy).toHaveBeenCalledWith(fullMessage);
  });

  it("degrades when Git cannot report the commit", async () => {
    mockFiles.mockRejectedValue(new Error("git exploded"));
    render("/workspace/broken");
    await flush();

    expect(tooltip().textContent).toContain("Changed files unavailable");
  });
});

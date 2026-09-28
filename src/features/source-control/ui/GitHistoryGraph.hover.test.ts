// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("../../../platform/tauri/fs", () => ({
  gitHistory: vi.fn(),
  gitCommitFiles: vi.fn(async () => []),
  gitCommitMessage: vi.fn(async () => "Subject\n"),
  subscribeGitChanged: vi.fn(() => () => undefined),
}));
vi.mock("../../../platform/tauri/clipboard", () => ({
  copyText: vi.fn(async () => undefined),
}));

import { gitHistory, type GitHistoryCommit } from "../../../platform/tauri/fs";
import { GitHistoryGraph } from "./GitHistoryGraph";

const commits: GitHistoryCommit[] = [
  {
    sha: "a".repeat(40),
    shortSha: "aaaaaaa",
    parents: ["b".repeat(40)],
    author: "Ada",
    timestamp: 1_780_000_000,
    subject: "First",
    refs: [],
    head: true,
  },
  {
    sha: "b".repeat(40),
    shortSha: "bbbbbbb",
    parents: [],
    author: "Ada",
    timestamp: 1_779_000_000,
    subject: "Second",
    refs: [],
    head: false,
  },
];

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.mocked(gitHistory).mockResolvedValue({ head: commits[0]!.sha, commits });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

it("tabs into the commit actions, back to the row, and forward to the next row", async () => {
  await act(async () => {
    root.render(
      createElement(GitHistoryGraph, {
        cwd: "/repo/hover-keyboard",
        enabled: true,
        expanded: true,
        onToggleExpanded: () => undefined,
        onOpenCommit: () => undefined,
      }),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

  const rows = container.querySelectorAll<HTMLButtonElement>("li > button");
  expect(rows).toHaveLength(2);
  act(() => rows[0]!.focus());
  let card = document.querySelector<HTMLElement>('[role="dialog"]')!;
  expect(card).not.toBeNull();

  act(() =>
    rows[0]!.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Tab",
        bubbles: true,
        cancelable: true,
      }),
    ),
  );
  expect(document.activeElement).toBe(card.querySelector("button"));

  act(() =>
    document.activeElement!.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Tab",
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      }),
    ),
  );
  expect(document.activeElement).toBe(rows[0]);

  card = document.querySelector<HTMLElement>('[role="dialog"]')!;
  const lastAction = card.querySelectorAll<HTMLButtonElement>("button")[1]!;
  act(() => lastAction.focus());
  act(() =>
    lastAction.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Tab",
        bubbles: true,
        cancelable: true,
      }),
    ),
  );
  expect(document.activeElement).toBe(rows[1]);

  act(() =>
    rows[1]!.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Tab",
        bubbles: true,
        cancelable: true,
      }),
    ),
  );
  act(() =>
    document.activeElement!.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    ),
  );
  expect(document.activeElement).toBe(rows[1]);
  expect(document.querySelector('[role="dialog"]')).toBeNull();
});

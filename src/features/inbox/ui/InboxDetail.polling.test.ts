// @vitest-environment happy-dom
import { invoke } from "@tauri-apps/api/core";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { clearInboxCache, type InboxItem } from "../model/githubTasks";
import { InboxDetail } from "./InboxView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const item: InboxItem = {
  provider: "github",
  kind: "pr",
  repo: "acme/web",
  number: 42,
  title: "Update sidebar",
  url: "https://github.com/acme/web/pull/42",
  state: "open",
  updatedAt: "2026-10-06T12:00:00Z",
  labels: [],
  assignees: [],
  draft: false,
  projectPath: "/tmp/web",
};

let root: Root;
let container: HTMLDivElement;
const checkRequests = () =>
  vi
    .mocked(invoke)
    .mock.calls.filter(([command]) => command === "git_github_pr_checks");

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.useFakeTimers();
  clearInboxCache();
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "git_github_pr_checks")
      return { headOid: "abc", checks: [] };
    if (command === "git_github_work_item_details")
      return { body: "", author: "" };
    if (command === "git_github_work_item_thread") {
      return { comments: [], commits: [], truncated: false };
    }
    throw new Error(`Unexpected command: ${command}`);
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it("refreshes CI while viewing Checks and stops after returning to Summary", async () => {
  await act(async () =>
    root.render(
      createElement(InboxDetail, {
        item,
        cwd: "/tmp/web",
        projects: [],
        revision: 0,
        relatedSessions: [],
        onDiscuss: () => {},
        onStart: () => {},
      }),
    ),
  );
  expect(checkRequests()).toHaveLength(1);
  await act(async () => vi.advanceTimersByTimeAsync(2 * 60_000));
  expect(checkRequests()).toHaveLength(1);

  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('[role="tab"][aria-label^="Checks:"]')!
      .click(),
  );
  expect(checkRequests()).toHaveLength(2);
  await act(async () => vi.advanceTimersByTimeAsync(30_000));
  expect(checkRequests()).toHaveLength(3);

  const summary = [
    ...container.querySelectorAll<HTMLButtonElement>('[role="tab"]'),
  ].find((button) => button.textContent === "Summary")!;
  await act(async () => summary.click());
  await act(async () => vi.advanceTimersByTimeAsync(2 * 60_000));
  expect(checkRequests()).toHaveLength(3);
});

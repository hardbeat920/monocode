// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("../../../platform/tauri/fs", () => ({
  gitHistory: vi.fn(),
  subscribeGitChanged: () => () => {},
}));

import { gitHistory, type GitHistory } from "../../../platform/tauri/fs";
import { GitHistoryGraph } from "./GitHistoryGraph";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.mocked(gitHistory).mockReset();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

function history(name: string): GitHistory {
  return {
    head: name,
    commits: [
      {
        sha: name,
        shortSha: name,
        parents: [],
        author: "Author",
        timestamp: 1_760_000_000,
        subject: `${name} commit`,
        refs: [{ name, kind: "local" }],
        head: true,
      },
    ],
  };
}

async function render(cwd: string, enabled = true) {
  await act(async () => {
    root.render(
      createElement(GitHistoryGraph, { cwd, enabled, onOpenCommit: vi.fn() }),
    );
  });
}

it("ignores history that arrives after switching projects", async () => {
  let resolveOld!: (result: GitHistory) => void;
  vi.mocked(gitHistory)
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveOld = resolve;
        }),
    )
    .mockResolvedValueOnce(history("current"));
  await render("/old-history-project");
  await render("/current-history-project");
  await act(async () => resolveOld(history("old")));
  expect(container.textContent).toContain("current commit");
  expect(container.textContent).not.toContain("old commit");
});

it("keeps loaded commits on refresh failure and clears the error after retry", async () => {
  vi.mocked(gitHistory).mockResolvedValueOnce(history("loaded"));
  await render("/refresh-history-project");
  const refresh = container.querySelector<HTMLButtonElement>(
    '[aria-label="Refresh history"]',
  )!;
  vi.mocked(gitHistory).mockRejectedValueOnce(new Error("History unavailable"));
  await act(async () => refresh.click());
  expect(container.querySelector('[role="alert"]')?.textContent).toBe(
    "History unavailable",
  );
  expect(container.textContent).toContain("loaded commit");
  vi.mocked(gitHistory).mockResolvedValueOnce(history("updated"));
  await act(async () => refresh.click());
  expect(container.querySelector('[role="alert"]')).toBeNull();
  expect(container.textContent).toContain("updated commit");
});

it("does not restore an in-flight result after the pane is disabled", async () => {
  let resolve!: (result: GitHistory) => void;
  vi.mocked(gitHistory).mockImplementationOnce(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  await render("/disabled-history-project");
  await render("/disabled-history-project", false);
  await act(async () => resolve(history("late")));
  expect(container.querySelector(".git-history-item")).toBeNull();
});

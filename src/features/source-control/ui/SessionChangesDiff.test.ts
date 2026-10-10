// @vitest-environment happy-dom
import { act, createElement, Fragment } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  notifyReviewChanged,
  type CheckpointFile,
} from "../../sessions/model/checkpoint";
import { SessionReview } from "../../sessions/ui/SessionReview";
import { SessionChangesDiff } from "./SessionChangesDiff";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("./UnifiedDiffView", () => ({
  UnifiedDiffView: ({
    files,
  }: {
    files: { label: string; additions: number; deletions: number }[];
  }) =>
    createElement(
      "div",
      { "data-review-files": true },
      files
        .map((file) => `${file.label}: +${file.additions} -${file.deletions}`)
        .join("\n"),
    ),
}));

let root: Root;
let container: HTMLDivElement;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.clearAllMocks();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

it.each(["/repo-worktrees/feature", "/repo-cow/session"])(
  "reviews only the current checkpoint after keeping an earlier turn in %s",
  async (cwd) => {
    const file = (relative: string): CheckpointFile => ({
      path: `${cwd}/${relative}`,
      relative,
      status: "modified",
      additions: 1,
      deletions: 1,
      exact: true,
      undoable: true,
    });
    let files = [file("first.txt")];
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "session_checkpoint_status") return { files };
      if (command === "session_checkpoint_file_diff")
        return {
          original: "inherited edit\nold\n",
          current: "inherited edit\nnew\n",
          binary: false,
          tooLarge: false,
        };
      if (command === "session_checkpoint_keep") {
        files = [];
        return { files };
      }
      throw new Error(`Unexpected command: ${command}`);
    });
    let reviewOpened = false;
    const render = () =>
      root.render(
        createElement(
          Fragment,
          null,
          createElement(SessionReview, {
            cwd,
            sessionId: "owner",
            onOpenDiff: () => {
              reviewOpened = true;
              render();
            },
          }),
          reviewOpened
            ? createElement(SessionChangesDiff, { cwd, sessionId: "owner" })
            : null,
        ),
      );
    await act(async () => render());
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('button[title="Review changes"]')!
        .click(),
    );
    expect(container.querySelector("[data-review-files]")?.textContent).toBe(
      "first.txt: +1 -1",
    );
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>(
          'button[title="Keep the recorded changes and dismiss this card"]',
        )!
        .click(),
    );
    await act(async () => notifyReviewChanged("owner"));
    expect(container.textContent).toBe("No session changes");
    files = [file("second.txt")];
    await act(async () => notifyReviewChanged("owner"));
    await vi.waitFor(() =>
      expect(container.textContent).toContain("second.txt"),
    );
    expect(container.textContent).not.toContain("first.txt");
    expect(container.querySelector("[data-review-files]")?.textContent).toBe(
      "second.txt: +1 -1",
    );
    expect(invoke).toHaveBeenCalledWith("session_checkpoint_file_diff", {
      sessionId: "owner",
      cwd,
      relative: "second.txt",
    });
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(([command]) => command.startsWith("cow_")),
    ).toBe(false);
  },
);

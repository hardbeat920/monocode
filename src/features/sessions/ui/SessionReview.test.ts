// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { CheckpointStatus } from "../model/checkpoint";
import { SessionReview } from "./SessionReview";

const probes = vi.hoisted(() => ({
  status: vi.fn(),
  keep: vi.fn(),
  undo: vi.fn(),
}));
vi.mock("../model/checkpoint", () => ({
  sessionCheckpointStatus: probes.status,
  keepSessionChanges: probes.keep,
  undoSessionChanges: probes.undo,
  subscribeReviewChanged: () => () => {},
}));
vi.mock("../../../platform/tauri/fs", () => ({
  basename: (path: string) => path.split("/").at(-1),
  subscribeGitChanged: () => () => {},
  notifyGitChanged: vi.fn(),
}));
vi.mock("../../files/model/fileWatch", () => ({
  invalidateWatchedFiles: vi.fn(),
}));
vi.mock("../../files/model/fileIndex", () => ({
  invalidateProjectFiles: vi.fn(),
}));
vi.mock("../../files/ui/FileTypeIcon", () => ({ FileTypeIcon: () => null }));

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  probes.status.mockReset();
  probes.keep.mockReset();
  probes.undo.mockReset();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

const status = (relative: string, undoable = true): CheckpointStatus => ({
  files: [
    {
      path: `/repo/${relative}`,
      relative,
      status: "modified",
      additions: 3,
      deletions: 1,
      exact: true,
      undoable,
    },
  ],
});

async function render(sessionId = "s", busy = false) {
  await act(async () => {
    root.render(
      createElement(SessionReview, {
        sessionId,
        cwd: "/repo",
        busy,
        onOpenDiff: vi.fn(),
      }),
    );
  });
}

it("keeps exact recorded counts visible when shared work makes Undo unavailable", async () => {
  probes.status.mockResolvedValue(status("shared.ts", false));
  await render();
  expect(container.textContent).toContain("shared.ts");
  expect(container.textContent).toContain("+3");
  expect(container.textContent).toContain("-1");
  expect(container.textContent).not.toContain("Mixed changes");
  const undo = [...container.querySelectorAll("button")].find(
    (button) => button.textContent === "Undo",
  )!;
  expect(undo.disabled).toBe(true);
  expect(
    [...container.querySelectorAll("button")].find(
      (button) => button.textContent === "Review",
    )!.disabled,
  ).toBe(false);
});

it("shows recording failures instead of implying there were no changes", async () => {
  probes.status.mockRejectedValue(
    new Error("Final checkpoint could not be captured"),
  );
  await render();
  expect(container.textContent).toContain("Couldn’t record changes");
  expect(container.textContent).toContain(
    "Final checkpoint could not be captured",
  );
  expect(container.querySelector("[data-session-review]")).toBeNull();
});

it("ignores a late load from a different session", async () => {
  let complete!: (status: CheckpointStatus) => void;
  probes.status.mockReturnValueOnce(
    new Promise<CheckpointStatus>((resolve) => {
      complete = resolve;
    }),
  );
  await render("old");
  probes.status.mockResolvedValue(status("current.ts"));
  await render("current");
  await act(async () => complete(status("old.ts")));
  expect(container.textContent).toContain("current.ts");
  expect(container.textContent).not.toContain("old.ts");
});

it("clears the previous session's files while the next session is loading", async () => {
  probes.status.mockResolvedValueOnce(status("old.ts"));
  await render("old");
  expect(container.textContent).toContain("old.ts");
  probes.status.mockReturnValue(new Promise(() => {}));
  await render("current");
  expect(container.textContent).toBe("");
});

it("hides the previous result during a live turn and clears it after a no-op turn", async () => {
  probes.status.mockResolvedValue(status("old.ts"));
  await render();
  await render("s", true);
  expect(container.textContent).toBe("");
  probes.status.mockResolvedValue({ files: [] });
  await render();
  expect(container.textContent).toBe("");
});

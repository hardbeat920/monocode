// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { CheckpointStatus } from "../model/checkpoint";
import { SessionChangesButton, SessionReview } from "./SessionReview";
import { notifyGitChanged } from "../../../platform/tauri/fs";

const probes = vi.hoisted(() => ({
  status: vi.fn(),
  keep: vi.fn(),
  undo: vi.fn(),
}));
vi.mock("../model/checkpoint", () => ({
  sessionCheckpointStatus: probes.status,
  keepSessionChanges: probes.keep,
  undoSessionChanges: probes.undo,
  subscribeReviewChanged: (listener: (id: string) => void) => {
    const handle = (event: Event) =>
      listener((event as CustomEvent<string>).detail);
    window.addEventListener("test-review-changed", handle);
    return () => window.removeEventListener("test-review-changed", handle);
  },
}));
vi.mock("../../../platform/tauri/fs", () => ({
  basename: (path: string) => path.split("/").at(-1),
  subscribeGitChanged: (listener: () => void) => {
    window.addEventListener("test-git-changed", listener);
    return () => window.removeEventListener("test-git-changed", listener);
  },
  notifyGitChanged: () => window.dispatchEvent(new Event("test-git-changed")),
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
let testId = 0;
const sessionKey = (id = "s") => `${testId}:${id}`;
beforeEach(() => {
  testId++;
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
  vi.useRealTimers();
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

async function render(
  sessionId = "s",
  busy = false,
  editedPaths?: string[],
  enabled = true,
  cwd = "/repo",
) {
  await act(async () => {
    root.render(
      createElement(SessionReview, {
        sessionId: sessionKey(sessionId),
        cwd,
        busy,
        editedPaths,
        enabled,
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

it("removes the card when a Git refresh reports that its files were committed", async () => {
  vi.useFakeTimers();
  probes.status.mockResolvedValue(status("committed.ts"));
  await render();
  expect(container.querySelector("[data-session-review]")).not.toBeNull();

  probes.status.mockResolvedValue({ files: [] });
  await act(async () => {
    notifyGitChanged();
    vi.advanceTimersByTime(200);
  });
  expect(probes.status).toHaveBeenCalledTimes(2);
  expect(container.querySelector("[data-session-review]")).toBeNull();
});

it("reloads with persisted edit evidence when the transcript becomes available", async () => {
  probes.status.mockResolvedValueOnce({ files: [] });
  await render();
  probes.status.mockResolvedValue(status("README.md", false));
  await render("s", false, ["README.md"]);
  expect(probes.status).toHaveBeenLastCalledWith(sessionKey(), "/repo", [
    "README.md",
  ]);
  expect(container.textContent).toContain("README.md");
  await render("s", false, ["README.md"]);
  expect(probes.status).toHaveBeenCalledTimes(2);
});

it("reuses the changes when switching away, returning, and refocusing the app", async () => {
  vi.useFakeTimers();
  probes.status.mockResolvedValue(status("cached.ts"));
  await render();
  await render("s", false, undefined, false);
  await render();
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
    document.dispatchEvent(new Event("visibilitychange"));
    vi.advanceTimersByTime(200);
  });
  expect(container.textContent).toContain("cached.ts");
  expect(probes.status).toHaveBeenCalledTimes(1);
});

it("reuses the changes after closing and reopening a session", async () => {
  probes.status.mockResolvedValue(status("reopened.ts"));
  await render();
  act(() => root.unmount());
  root = createRoot(container);
  await render();
  expect(container.textContent).toContain("reopened.ts");
  expect(probes.status).toHaveBeenCalledTimes(1);
});

it("caches an empty result and keeps separate results for different workspaces", async () => {
  probes.status.mockResolvedValueOnce({ files: [] });
  await render();
  probes.status.mockResolvedValue(status("elsewhere.ts"));
  await render("s", false, undefined, true, "/other");
  expect(container.textContent).toContain("elsewhere.ts");
  await render();
  expect(container.textContent).toBe("");
  expect(probes.status).toHaveBeenCalledTimes(2);
});

it("shares a pending request when a session closes and reopens before it finishes", async () => {
  let complete!: (status: CheckpointStatus) => void;
  probes.status.mockReturnValue(
    new Promise<CheckpointStatus>((resolve) => {
      complete = resolve;
    }),
  );
  await render();
  act(() => root.unmount());
  root = createRoot(container);
  await render();
  await act(async () => complete(status("pending.ts")));
  expect(container.textContent).toContain("pending.ts");
  expect(probes.status).toHaveBeenCalledTimes(1);
});

it("remembers Git changes while the session is closed and refreshes only when reopened", async () => {
  vi.useFakeTimers();
  probes.status.mockResolvedValue(status("committed.ts"));
  await render();
  act(() => root.unmount());
  probes.status.mockResolvedValue({ files: [] });
  await act(async () => {
    notifyGitChanged();
    vi.advanceTimersByTime(200);
  });
  expect(probes.status).toHaveBeenCalledTimes(1);
  root = createRoot(container);
  await render();
  expect(probes.status).toHaveBeenCalledTimes(2);
  expect(container.querySelector("[data-session-review]")).toBeNull();
});

it("ignores another thread's updates and defers a hidden live turn's refresh until it settles", async () => {
  vi.useFakeTimers();
  probes.status.mockResolvedValue(status("before.ts"));
  await render();
  await render("s", false, undefined, false);
  await act(async () => {
    window.dispatchEvent(
      new CustomEvent("test-review-changed", { detail: sessionKey("other") }),
    );
    vi.advanceTimersByTime(200);
  });
  await render();
  expect(probes.status).toHaveBeenCalledTimes(1);

  await render("s", true, undefined, false);
  probes.status.mockResolvedValue(status("continued.ts"));
  await act(async () => {
    window.dispatchEvent(
      new CustomEvent("test-review-changed", { detail: sessionKey() }),
    );
    notifyGitChanged();
    vi.advanceTimersByTime(200);
  });
  expect(probes.status).toHaveBeenCalledTimes(1);
  await render();
  expect(container.textContent).toContain("continued.ts");
  expect(probes.status).toHaveBeenCalledTimes(2);
});

it("does not let an older request restore committed files in the cache", async () => {
  vi.useFakeTimers();
  let complete!: (status: CheckpointStatus) => void;
  probes.status.mockReturnValueOnce(
    new Promise<CheckpointStatus>((resolve) => {
      complete = resolve;
    }),
  );
  await render();
  probes.status.mockResolvedValue({ files: [] });
  await act(async () => {
    notifyGitChanged();
    vi.advanceTimersByTime(200);
  });
  await act(async () => complete(status("committed.ts")));
  act(() => root.unmount());
  root = createRoot(container);
  await render();
  expect(container.textContent).toBe("");
  expect(probes.status).toHaveBeenCalledTimes(2);
});

it("updates the cache directly after Keep and reuses the cleared result on reopening", async () => {
  vi.useFakeTimers();
  probes.status.mockResolvedValue(status("kept.ts"));
  probes.keep.mockResolvedValue({ files: [] });
  await render();
  const keep = [...container.querySelectorAll("button")].find(
    (button) => button.textContent === "Keep",
  )!;
  await act(async () => keep.click());
  await act(async () => vi.advanceTimersByTime(200));
  act(() => root.unmount());
  root = createRoot(container);
  await render();
  expect(container.textContent).toBe("");
  expect(probes.status).toHaveBeenCalledTimes(1);
});

it("keeps the card visible during a refresh and shares its cache with the compact button", async () => {
  vi.useFakeTimers();
  probes.status.mockResolvedValue(status("before.ts"));
  await render();
  let complete!: (status: CheckpointStatus) => void;
  probes.status.mockReturnValue(
    new Promise<CheckpointStatus>((resolve) => {
      complete = resolve;
    }),
  );
  await act(async () => {
    window.dispatchEvent(
      new CustomEvent("test-review-changed", { detail: sessionKey() }),
    );
    vi.advanceTimersByTime(200);
  });
  expect(container.textContent).toContain("before.ts");
  await act(async () => complete(status("after.ts")));
  act(() => root.unmount());
  root = createRoot(container);
  await act(async () =>
    root.render(
      createElement(SessionChangesButton, {
        sessionId: sessionKey(),
        cwd: "/repo",
        onOpenDiff: vi.fn(),
      }),
    ),
  );
  expect(container.querySelector("[data-session-changes]")).not.toBeNull();
  expect(probes.status).toHaveBeenCalledTimes(2);
});

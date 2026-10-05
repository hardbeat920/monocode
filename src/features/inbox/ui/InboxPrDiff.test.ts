// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { InboxPrDiff } from "./InboxPrDiff";
import { highlightDiffFile } from "../../files/editor/syntaxTokens";

vi.mock("../../files/editor/syntaxTokens", () => ({
  highlightDiffFile: vi.fn(() => Promise.resolve(null)),
}));

let container: HTMLDivElement;
let root: Root;
const diff = {
  files: [
    { path: "README.md", additions: 1, deletions: 0 },
    { path: "package.json", additions: 1, deletions: 1 },
  ],
  patch:
    "diff --git a/README.md b/README.md\n--- /dev/null\n+++ b/README.md\n@@ -0,0 +1 @@\n+ready\n",
  additions: 2,
  deletions: 1,
  truncated: true,
};
const contents = (text: string) => ({
  original: "old\n",
  current: `${text}\n`,
  binary: false,
  tooLarge: false,
});

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  vi.mocked(highlightDiffFile).mockClear();
});
afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});
async function togglePackage() {
  const button = container.querySelector<HTMLButtonElement>(
    '[data-diff-file="package.json"] header button',
  );
  await act(async () => button!.click());
}

it("loads a missing file on expansion, preserves totals and reuses its contents", async () => {
  let resolve!: (value: ReturnType<typeof contents>) => void;
  const loadFile = vi.fn(
    () =>
      new Promise<ReturnType<typeof contents>>((done) => {
        resolve = done;
      }),
  );
  await act(async () =>
    root.render(createElement(InboxPrDiff, { diff, loadFile })),
  );
  expect(loadFile).not.toHaveBeenCalled();
  const readmeHighlights = () =>
    vi
      .mocked(highlightDiffFile)
      .mock.calls.filter(([file]) => file.id === "README.md").length;
  const initialHighlights = readmeHighlights();
  expect(initialHighlights).toBeGreaterThan(0);
  await togglePackage();
  expect(loadFile).toHaveBeenCalledExactlyOnceWith("package.json");
  expect(container.textContent).toContain("Loading diff");
  await act(async () => resolve(contents("loaded package")));
  expect(container.textContent).toContain("loaded package");
  expect(container.textContent).toContain("2 files");
  expect(readmeHighlights()).toBe(initialHighlights);
  await togglePackage();
  await togglePackage();
  expect(loadFile).toHaveBeenCalledTimes(1);
});

it("retries failed loads and does not reuse contents after the MR snapshot changes", async () => {
  const loadFile = vi
    .fn()
    .mockRejectedValueOnce(new Error("Could not reach GitLab"))
    .mockResolvedValueOnce(contents("old snapshot"))
    .mockResolvedValueOnce(contents("new snapshot"));
  await act(async () =>
    root.render(createElement(InboxPrDiff, { diff, loadFile })),
  );
  await togglePackage();
  expect(container.textContent).toContain("Could not reach GitLab");
  await togglePackage();
  await togglePackage();
  expect(container.textContent).toContain("old snapshot");
  await act(async () =>
    root.render(createElement(InboxPrDiff, { diff: { ...diff }, loadFile })),
  );
  expect(container.textContent).toContain("new snapshot");
  expect(container.textContent).not.toContain("old snapshot");
  expect(loadFile).toHaveBeenCalledTimes(3);
});

it("keeps the existing preview behavior when no file loader is provided", async () => {
  await act(async () => root.render(createElement(InboxPrDiff, { diff })));
  expect(container.textContent).toContain("ready");
  await togglePackage();
  expect(container.textContent).toContain(
    "Patch unavailable because this change is too large",
  );
  expect(container.textContent).not.toContain("Loading diff");
  expect(container.textContent).toContain("2 files");
});

it("ignores an older preview's response that finishes after the current file", async () => {
  let resolveOld!: (value: ReturnType<typeof contents>) => void;
  const loadFile = vi
    .fn()
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveOld = resolve;
        }),
    )
    .mockResolvedValueOnce(contents("current snapshot"));
  await act(async () =>
    root.render(createElement(InboxPrDiff, { diff, loadFile })),
  );
  await togglePackage();
  await act(async () =>
    root.render(createElement(InboxPrDiff, { diff: { ...diff }, loadFile })),
  );
  expect(container.textContent).toContain("current snapshot");
  await act(async () => resolveOld(contents("obsolete snapshot")));
  expect(container.textContent).toContain("current snapshot");
  expect(container.textContent).not.toContain("obsolete snapshot");
  expect(loadFile).toHaveBeenCalledTimes(2);
});

// @vitest-environment happy-dom
import {
  act,
  createElement,
  useEffect,
  useState,
  type ComponentProps,
} from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  newChangesTab,
  newCommitTab,
  newEditorPane,
  newFileTab,
  newSessionChangesTab,
} from "../../workspace/model/layout";

const lifecycle = vi.hoisted(() => ({
  mounts: new Map<string, number>(),
  unmounts: new Map<string, number>(),
}));
function Review({ name, visible }: { name: string; visible?: boolean }) {
  const [fold, setFold] = useState("open");
  useEffect(() => {
    lifecycle.mounts.set(name, (lifecycle.mounts.get(name) ?? 0) + 1);
    return () => {
      lifecycle.unmounts.set(name, (lifecycle.unmounts.get(name) ?? 0) + 1);
    };
  }, [name]);
  return createElement(
    "div",
    { "data-review": name, "data-visible": String(visible) },
    createElement("button", { onClick: () => setFold("closed") }, fold),
  );
}
vi.mock("../../source-control/ui/WorkingTreeDiff", () => ({
  WorkingTreeDiff: ({ visible }: { visible: boolean }) =>
    createElement(Review, { name: "working", visible }),
}));
vi.mock("../../source-control/ui/CommitDiff", () => ({
  CommitDiff: ({ sha, visible }: { sha: string; visible: boolean }) =>
    createElement(Review, { name: `commit:${sha}`, visible }),
}));
vi.mock("../../source-control/ui/SessionChangesDiff", () => ({
  SessionChangesDiff: ({ visible }: { visible: boolean }) =>
    createElement(Review, { name: "session", visible }),
}));
vi.mock("./FileEditor", () => ({
  FileEditor: () => createElement("div", { "data-editor": true }),
}));
vi.mock("../../workspace/ui/SurfaceTabs", () => ({ SurfaceTabs: () => null }));
import { FilePane } from "./FilePane";

let host: HTMLDivElement;
let root: Root;
let props: ComponentProps<typeof FilePane>;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const storage = new Map([["monocode.diffViewer", "unified"]]);
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
  });
  lifecycle.mounts.clear();
  lifecycle.unmounts.clear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  props = {
    pane: newEditorPane(newFileTab("/repo/editor.ts", "/repo")),
    focused: true,
    showTabs: false,
    dirtyFileIds: new Set(),
    fileErrorCounts: new Map(),
    sessions: [],
    onFocus: vi.fn(),
    onSelectFile: vi.fn(),
    onCloseFile: vi.fn(),
    onCloseOtherFiles: vi.fn(),
    onDirtyChange: vi.fn(),
    onErrorCountChange: vi.fn(),
    onReorderFiles: vi.fn(),
    onOpenFile: vi.fn(),
    onUpdatePlan: vi.fn(),
    onBuildPlan: vi.fn(),
  };
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});
async function render() {
  await act(async () => root.render(createElement(FilePane, props)));
  await act(async () => {
    await vi.dynamicImportSettled();
  });
}

it.each([
  ["working", () => newChangesTab("/repo")],
  ["working", () => newFileTab("/repo/a.ts", "/repo", true)],
  [
    "commit:abc",
    () =>
      newCommitTab("/repo", { sha: "abc", shortSha: "abc", subject: "commit" }),
  ],
  ["session", () => newSessionChangesTab("/repo", "s1")],
] as const)(
  "preserves an open %s review across editor and workspace navigation",
  async (name, createReview) => {
    const review = createReview();
    const editor = newFileTab("/repo/editor.ts", "/repo");
    props.pane = {
      ...props.pane,
      files: [review, editor],
      activeFileId: review.id,
    };
    await render();
    const surface = host.querySelector<HTMLElement>(`[data-review="${name}"]`)!;
    expect(surface).not.toBeNull();
    await act(async () => surface.querySelector("button")!.click());
    surface.scrollTop = 123;
    props = { ...props, pane: { ...props.pane, activeFileId: editor.id } };
    await render();
    expect(surface.dataset.visible).toBe("false");
    expect(lifecycle.unmounts.get(name) ?? 0).toBe(0);
    expect(host.querySelectorAll("[data-editor]")).toHaveLength(1);
    props = { ...props, pane: { ...props.pane, activeFileId: review.id } };
    await render();
    expect(host.querySelector(`[data-review="${name}"]`)).toBe(surface);
    expect(surface.textContent).toBe("closed");
    expect(surface.scrollTop).toBe(123);
    props = { ...props, visible: false };
    await render();
    expect(surface.dataset.visible).toBe("false");
    props = { ...props, visible: true };
    await render();
    expect(surface.dataset.visible).toBe("true");
    expect(lifecycle.mounts.get(name)).toBe(1);
    props = {
      ...props,
      pane: { ...props.pane, files: [editor], activeFileId: editor.id },
    };
    await render();
    expect(lifecycle.unmounts.get(name)).toBe(1);
  },
);

it("keeps different commit tabs as separate review instances", async () => {
  const first = newCommitTab("/repo", {
    sha: "a",
    shortSha: "a",
    subject: "first",
  });
  const second = newCommitTab("/repo", {
    sha: "b",
    shortSha: "b",
    subject: "second",
  });
  props.pane = {
    ...props.pane,
    files: [first, second],
    activeFileId: first.id,
  };
  await render();
  const a = host.querySelector('[data-review="commit:a"]')!;
  await act(async () => a.querySelector("button")!.click());
  props = { ...props, pane: { ...props.pane, activeFileId: second.id } };
  await render();
  expect(host.querySelector('[data-review="commit:b"]')!.textContent).toBe(
    "open",
  );
  props = { ...props, pane: { ...props.pane, activeFileId: first.id } };
  await render();
  expect(a.textContent).toBe("closed");
  expect(lifecycle.mounts.get("commit:a")).toBe(1);
});

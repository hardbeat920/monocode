// @vitest-environment happy-dom
import { EditorView } from "@codemirror/view";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { newEditorPane, newFileTab } from "../lib/layout";
import { invalidateWatchedFiles } from "../lib/fileWatch";
import { FilePane } from "./FilePane";

const invoke = vi.hoisted(() =>
  vi.fn(async (command: string) => {
    if (command === "read_text_file")
      return "first line\nsecond line\nthird line";
    if (command === "git_file_diff")
      return {
        original: "first line\nold line\nthird line",
        current: "first line\nsecond line\nthird line",
        binary: false,
        tooLarge: false,
      };
    if (command === "stat_files") return [];
    throw new Error(`Unexpected command: ${command}`);
  }),
);
vi.mock("@tauri-apps/api/core", async (original) => ({
  ...(await original<typeof import("@tauri-apps/api/core")>()),
  invoke,
}));

describe("file pane source navigation", () => {
  let root: Root;
  let container: HTMLDivElement;

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const storage = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => {
        storage.set(key, value);
      },
    });
    invoke.mockClear();
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });

  async function render(path: string, line: number, review = false) {
    const pane = newEditorPane(newFileTab(path, "/repo", review));
    await act(async () =>
      root.render(
        createElement(FilePane, {
          pane,
          focused: true,
          dirtyFileIds: new Set<string>(),
          fileErrorCounts: new Map<string, number>(),
          sessions: [],
          onFocus: () => {},
          onSelectFile: () => {},
          onCloseFile: () => {},
          onDirtyChange: () => {},
          onErrorCountChange: () => {},
          onReorderFiles: () => {},
          onOpenFile: () => {},
          onUpdatePlan: () => {},
          onBuildPlan: () => {},
          editorNavigation: { path, line, column: 2, token: 1 },
        }),
      ),
    );
    await act(async () =>
      vi.waitFor(() =>
        expect(container.querySelector(".cm-editor")).not.toBeNull(),
      ),
    );
    return EditorView.findFromDOM(
      container.querySelector<HTMLElement>(".cm-editor")!,
    )!;
  }

  it("shows Markdown source and navigates to its referenced line", async () => {
    const view = await render("/repo/navigation.md", 2);
    await act(async () =>
      vi.waitFor(() => expect(view.state.selection.main.head).toBe(12)),
    );
    expect(
      container.querySelector(
        '[aria-label="Markdown view"] [role="tab"][aria-selected="true"]',
      )?.textContent,
    ).toBe("Source");
    expect(view.state.doc.toString()).toBe(
      "first line\nsecond line\nthird line",
    );
    const preview = [
      ...container.querySelectorAll<HTMLButtonElement>('[role="tab"]'),
    ].find((tab) => tab.textContent === "Preview")!;
    await act(async () => preview.click());
    expect(preview.getAttribute("aria-selected")).toBe("true");
  });

  it("clamps a stale source location to the last line instead of waiting forever", async () => {
    const view = await render("/repo/short.txt", 999);
    await act(async () =>
      vi.waitFor(() =>
        expect(
          view.state.doc.lineAt(view.state.selection.main.head).number,
        ).toBe(3),
      ),
    );
  });

  it("reapplies the requested location when a pending file reload adds its line", async () => {
    invoke.mockResolvedValueOnce("first line");
    const view = await render("/repo/growing.txt", 3);
    await act(async () =>
      vi.waitFor(() => {
        expect(view.state.doc.lines).toBe(1);
        expect(view.state.selection.main.head).toBe(1);
      }),
    );
    await act(async () => {
      invalidateWatchedFiles(["/repo/growing.txt"]);
      await new Promise((resolve) => setTimeout(resolve, 100));
    });
    await act(async () =>
      vi.waitFor(() => {
        expect(view.state.doc.lines).toBe(3);
        expect(
          view.state.doc.lineAt(view.state.selection.main.head).number,
        ).toBe(3);
        expect(view.state.selection.main.head).toBe(
          view.state.doc.line(3).from + 1,
        );
      }),
    );
  });

  it("keeps file contents and the existing added/removed-line diff in the same pane", async () => {
    const view = await render("/repo/review.txt", 2, true);
    expect(view.state.doc.toString()).toContain("second line");
    await act(async () =>
      vi.waitFor(() => {
        expect(container.textContent).toContain("+1");
        expect(container.textContent).toContain("-1");
      }),
    );
    expect(invoke).toHaveBeenCalledWith("git_file_diff", {
      cwd: "/repo",
      relative: "review.txt",
      staged: false,
    });
  });
});

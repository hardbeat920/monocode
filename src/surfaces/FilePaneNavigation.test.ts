// @vitest-environment happy-dom
import { EditorView } from "@codemirror/view";
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { newEditorPane, newFileTab } from "../lib/layout";
import { invalidateWatchedFiles } from "../lib/fileWatch";
import { FilePane } from "./FilePane";

const disk = vi.hoisted(() => ({ content: "" }));
const invoke = vi.hoisted(() =>
  vi.fn(async (command: string) => {
    if (command === "read_text_file") return disk.content;
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
  let paneProps: ComponentProps<typeof FilePane>;

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
    disk.content = "first line\nsecond line\nthird line";
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  async function render(path: string, line: number, review = false) {
    const pane = newEditorPane(newFileTab(path, "/repo", review));
    paneProps = {
      pane,
      focused: true,
      dirtyFileIds: new Set<string>(),
      fileErrorCounts: new Map<string, number>(),
      sessions: [],
      onFocus: () => {},
      onSelectFile: () => {},
      onCloseFile: () => {},
      onCloseOtherFiles: () => {},
      onDirtyChange: () => {},
      onErrorCountChange: () => {},
      onReorderFiles: () => {},
      onOpenFile: () => {},
      onUpdatePlan: () => {},
      onBuildPlan: () => {},
      editorNavigation: { path, line, column: 2, token: 1 },
    };
    await act(async () => root.render(createElement(FilePane, paneProps)));
    await act(async () =>
      vi.waitFor(() =>
        expect(container.querySelector(".cm-editor")).not.toBeNull(),
      ),
    );
    return EditorView.findFromDOM(
      container.querySelector<HTMLElement>(".cm-editor")!,
    )!;
  }

  async function reload(path: string, view: EditorView, content: string) {
    disk.content = content;
    await act(async () => {
      invalidateWatchedFiles([path]);
      await new Promise((resolve) => setTimeout(resolve, 100));
    });
    expect(view.state.doc.toString()).toBe(content);
    await act(async () => {
      // Flush navigation scheduled after the document replacement.
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      );
    });
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

  it("preserves a manually moved selection and external focus after a reload", async () => {
    const path = "/repo/moved.txt";
    const view = await render(path, 2);
    await act(async () =>
      vi.waitFor(() => expect(view.state.selection.main.head).toBe(12)),
    );
    const anchor = view.state.doc.line(3).from;
    const button = document.createElement("button");
    container.append(button);
    await act(async () => {
      view.dispatch({ selection: { anchor } });
      button.focus();
    });
    const focus = vi.spyOn(view, "focus");

    await reload(path, view, `${view.state.doc}\nfourth line`);

    expect(view.state.selection.main.head).toBe(anchor);
    expect(focus).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(button);
  });

  it("does not replay completed navigation even when the user has not moved", async () => {
    const path = "/repo/completed.txt";
    const view = await render(path, 2);
    await act(async () =>
      vi.waitFor(() => expect(view.state.selection.main.head).toBe(12)),
    );
    const focus = vi.spyOn(view, "focus");

    await reload(path, view, `${view.state.doc}\nfourth line`);

    expect(view.state.selection.main.head).toBe(12);
    expect(focus).not.toHaveBeenCalled();
  });

  it.each(["selection", "blur"])(
    "cancels a clamped pending navigation on %s before its line arrives",
    async (interaction) => {
      const path = `/repo/pending-${interaction}.txt`;
      invoke.mockResolvedValueOnce("first line");
      const view = await render(path, 3);
      await act(async () =>
        vi.waitFor(() => expect(view.state.selection.main.head).toBe(1)),
      );
      const button = document.createElement("button");
      container.append(button);
      await act(async () => {
        if (interaction === "selection")
          view.dispatch({ selection: { anchor: 0 } });
        else button.focus();
      });
      const focus = vi.spyOn(view, "focus");

      await reload(path, view, "first line\nsecond line\nthird line");

      expect(view.state.selection.main.head).toBe(
        interaction === "selection" ? 0 : 1,
      );
      expect(focus).not.toHaveBeenCalled();
      if (interaction === "blur") expect(document.activeElement).toBe(button);
    },
  );

  it("allows a new navigation token to revisit the same location", async () => {
    const path = "/repo/revisit.txt";
    const view = await render(path, 2);
    await act(async () =>
      vi.waitFor(() => expect(view.state.selection.main.head).toBe(12)),
    );
    await act(async () => view.dispatch({ selection: { anchor: 0 } }));
    await act(async () =>
      root.render(
        createElement(FilePane, {
          ...paneProps,
          editorNavigation: { path, line: 2, column: 2, token: 2 },
        }),
      ),
    );
    await act(async () =>
      vi.waitFor(() => expect(view.state.selection.main.head).toBe(12)),
    );
  });
});

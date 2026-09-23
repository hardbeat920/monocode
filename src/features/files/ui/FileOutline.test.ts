// @vitest-environment happy-dom
import { EditorView } from "@codemirror/view";
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { newEditorPane, newFileTab } from "../../workspace/model/layout";
import { OUTLINE_VIEW_DEFAULT, saveOutlineView } from "../model/outlineView";
import { FilePane } from "./FilePane";

const disk = vi.hoisted(() => ({ content: "" }));
const invoke = vi.hoisted(() =>
  vi.fn(async (command: string) => {
    if (command === "read_text_file") return disk.content;
    if (command === "stat_files") return [];
    if (command === "git_file_diff")
      return { original: "", current: "", binary: false, tooLarge: false };
    throw new Error(`Unexpected command: ${command}`);
  }),
);
vi.mock("@tauri-apps/api/core", async (original) => ({
  ...(await original<typeof import("@tauri-apps/api/core")>()),
  invoke,
}));

const SOURCE = [
  "function alpha() {",
  "  return 1;",
  "}",
  "class Beta {",
  "  method() {",
  "    return 2;",
  "  }",
  "}",
].join("\n");

describe("code view outline", () => {
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
    saveOutlineView(OUTLINE_VIEW_DEFAULT);
    invoke.mockClear();
    disk.content = SOURCE;
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

  async function render(path: string) {
    const pane = newEditorPane(newFileTab(path, "/repo", false));
    const props: ComponentProps<typeof FilePane> = {
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
    };
    await act(async () => root.render(createElement(FilePane, props)));
    await act(async () =>
      vi.waitFor(() =>
        expect(container.querySelector(".cm-editor")).not.toBeNull(),
      ),
    );
    return EditorView.findFromDOM(
      container.querySelector<HTMLElement>(".cm-editor")!,
    )!;
  }

  it("shows the file path in a top bar", async () => {
    await render("/repo/src/sample.ts");
    const bar = container.querySelector('[aria-label="File path"]');
    expect(bar?.textContent).toBe("src/sample.ts");
  });

  it("toggles the outline and navigates on selection", async () => {
    const view = await render("/repo/sample.ts");

    const toggle = container.querySelector<HTMLButtonElement>(
      'button[title="Show outline"]',
    );
    expect(toggle).not.toBeNull();
    await act(async () => toggle!.click());

    await act(async () =>
      vi.waitFor(() => {
        const labels = [
          ...container.querySelectorAll<HTMLElement>('[role="treeitem"]'),
        ].map((item) => item.textContent);
        expect(labels).toEqual(["alpha", "Beta", "method"]);
      }),
    );

    const method = [
      ...container.querySelectorAll<HTMLButtonElement>('[role="treeitem"]'),
    ].find((item) => item.textContent === "method")!;
    await act(async () => method.click());

    expect(view.state.doc.lineAt(view.state.selection.main.head).number).toBe(
      5,
    );
  });

  it("detaches the outline into a floating card and docks it back", async () => {
    await render("/repo/sample.ts");
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('button[title="Show outline"]')!
        .click(),
    );
    await act(async () =>
      vi.waitFor(() =>
        expect(
          container.querySelectorAll('[role="treeitem"]').length,
        ).toBeGreaterThan(0),
      ),
    );

    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('button[title="Detach outline"]')!
        .click(),
    );
    await act(async () =>
      vi.waitFor(() =>
        expect(
          container.querySelector('[role="dialog"][aria-label="Outline"]'),
        ).not.toBeNull(),
      ),
    );
    expect(container.querySelector('aside[aria-label="Outline"]')).toBeNull();

    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('button[title="Dock outline"]')!
        .click(),
    );
    await act(async () =>
      vi.waitFor(() =>
        expect(
          container.querySelector('aside[aria-label="Outline"]'),
        ).not.toBeNull(),
      ),
    );
  });
});

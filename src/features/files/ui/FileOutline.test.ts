// @vitest-environment happy-dom
import { EditorView } from "@codemirror/view";
import { Fragment, act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { newEditorPane, newFileTab } from "../../workspace/model/layout";
import {
  OUTLINE_VIEW_DEFAULT,
  OUTLINE_WIDTH_MAX,
  saveOutlineView,
} from "../model/outlineView";
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
// Resolve the language synchronously so the outline does not wait on a dynamic
// import, which is slow and flaky under CI load.
vi.mock("../editor/editorChrome", async (original) => {
  const actual =
    await original<typeof import("../editor/editorChrome")>();
  const { javascript } = await import("@codemirror/lang-javascript");
  return {
    ...actual,
    languageForPath: async (path: string) =>
      path.endsWith(".ts") || path.endsWith(".tsx")
        ? javascript({ typescript: true })
        : null,
  };
});

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

  async function renderPanes(
    entries: { path: string; focused: boolean }[],
  ) {
    const nodes = entries.map(({ path, focused }, index) => {
      const pane = newEditorPane(newFileTab(path, "/repo", false));
      const props: ComponentProps<typeof FilePane> = {
        pane,
        focused,
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
      return createElement(FilePane, { key: index, ...props });
    });
    await act(async () =>
      root.render(createElement(Fragment, null, ...nodes)),
    );
    await act(async () =>
      vi.waitFor(() =>
        expect(container.querySelectorAll(".cm-editor").length).toBe(
          entries.length,
        ),
      ),
    );
    return Array.from(
      container.querySelectorAll<HTMLElement>(".cm-editor"),
    ).map((el) => EditorView.findFromDOM(el)!);
  }

  async function render(path: string) {
    const [view] = await renderPanes([{ path, focused: true }]);
    return view;
  }

  function setClientWidth(el: HTMLElement, width: number) {
    Object.defineProperty(el, "clientWidth", {
      configurable: true,
      get: () => width,
    });
  }

  function pointer(target: EventTarget, type: string, clientX: number) {
    act(() =>
      target.dispatchEvent(
        new PointerEvent(type, {
          bubbles: true,
          cancelable: true,
          button: 0,
          pointerId: 1,
          clientX,
          clientY: 0,
        }),
      ),
    );
  }

  async function dragOutline(handle: HTMLElement, fromX: number, toX: number) {
    const captured = handle as unknown as {
      setPointerCapture: (id: number) => void;
      releasePointerCapture: (id: number) => void;
    };
    captured.setPointerCapture = () => {};
    captured.releasePointerCapture = () => {};
    pointer(handle, "pointerdown", fromX);
    pointer(window, "pointermove", toX);
    pointer(window, "pointerup", toX);
    await act(async () => {});
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
      vi.waitFor(
        () => {
          const labels = [
            ...container.querySelectorAll<HTMLElement>('[role="treeitem"]'),
          ].map((item) => item.textContent);
          expect(labels).toEqual(["alpha", "Beta", "method"]);
        },
        { timeout: 5000 },
      ),
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
      vi.waitFor(
        () =>
          expect(
            container.querySelectorAll('[role="treeitem"]').length,
          ).toBeGreaterThan(0),
        { timeout: 5000 },
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

  it("caps a narrow pane's outline at a fraction of its containing width", async () => {
    saveOutlineView({ ...OUTLINE_VIEW_DEFAULT, open: true });
    await render("/repo/sample.ts");
    const handle = container.querySelector<HTMLElement>(
      '[aria-label="Resize outline"]',
    )!;
    const aside = container.querySelector<HTMLElement>(
      'aside[aria-label="Outline"]',
    )!;
    setClientWidth(aside.parentElement!, 300);

    await dragOutline(handle, 300, 100);

    expect(aside.style.width).toBe("180px");
  });

  it("caps a wide pane's outline at OUTLINE_WIDTH_MAX", async () => {
    saveOutlineView({ ...OUTLINE_VIEW_DEFAULT, open: true });
    await render("/repo/sample.ts");
    const handle = container.querySelector<HTMLElement>(
      '[aria-label="Resize outline"]',
    )!;
    const aside = container.querySelector<HTMLElement>(
      'aside[aria-label="Outline"]',
    )!;
    setClientWidth(aside.parentElement!, 2000);

    await dragOutline(handle, 2000, 0);

    expect(aside.style.width).toBe(`${OUTLINE_WIDTH_MAX}px`);
  });

  it("resizes each split pane against its own containing width", async () => {
    saveOutlineView({ ...OUTLINE_VIEW_DEFAULT, open: true });
    await renderPanes([
      { path: "/repo/a.ts", focused: true },
      { path: "/repo/b.ts", focused: false },
    ]);
    const handles = container.querySelectorAll<HTMLElement>(
      '[aria-label="Resize outline"]',
    );
    const asides = container.querySelectorAll<HTMLElement>(
      'aside[aria-label="Outline"]',
    );
    setClientWidth(asides[0]!.parentElement!, 300);
    setClientWidth(asides[1]!.parentElement!, 800);

    await dragOutline(handles[0]!, 300, 0);
    await dragOutline(handles[1]!, 800, 0);

    expect(asides[0]!.style.width).toBe("180px");
    expect(asides[1]!.style.width).toBe("480px");
  });

  it("renders the detached outline only for the focused split pane", async () => {
    saveOutlineView({
      ...OUTLINE_VIEW_DEFAULT,
      open: true,
      detached: true,
    });
    await renderPanes([
      { path: "/repo/a.ts", focused: true },
      { path: "/repo/b.ts", focused: false },
    ]);

    expect(
      container.querySelectorAll('[role="dialog"][aria-label="Outline"]'),
    ).toHaveLength(1);
  });
});

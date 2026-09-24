// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { saveShowExcludedFiles } from "../../settings/model/appearance";
import {
  listCachedDir,
  notifyDirsChanged,
  refreshDir,
  saveExpanded,
  saveSelected,
} from "../model/fileTree";
import type { FsEntry } from "../../../platform/tauri/fs";
import {
  EXPLORER_FILE_POINTER_DRAG_EVENT,
  type ExplorerFilePointerDragDetail,
} from "../../../shared/lib/drag";
import { FileTree } from "./FileTree";

const { iconRender, directories, clipboardFiles, copied, dragDrop } =
  vi.hoisted(() => ({
    iconRender: vi.fn(),
    directories: new Map<string, FsEntry[]>(),
    clipboardFiles: [] as string[],
    copied: [] as { from: string; destParent: string }[],
    dragDrop: {
      handler: null as null | ((event: { payload: unknown }) => void),
    },
  }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args: Record<string, string>) => {
    if (command === "list_dir") return directories.get(args.path) ?? [];
    if (command === "clipboard_file_paths") return [...clipboardFiles];
    if (command === "copy_path") {
      copied.push({ from: args.from, destParent: args.destParent });
      return `${args.destParent}/${args.from.split("/").pop()}`;
    }
    throw new Error(`Unexpected command: ${command}`);
  }),
}));

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: async (fn: (event: { payload: unknown }) => void) => {
      dragDrop.handler = fn;
      return () => {
        dragDrop.handler = null;
      };
    },
  }),
}));

// Count row renders independently of FileTypeIcon's own memoization.
vi.mock("./FileTypeIcon", () => ({
  FileTypeIcon: ({ name }: { name: string }) => {
    iconRender(name);
    return createElement("span", { "data-icon": name });
  },
}));

let container: HTMLDivElement;
let root: Root;
let cwd: string;
let props: ComponentProps<typeof FileTree>;
let project = 0;

function file(name: string, ignored = false): FsEntry {
  return { name, path: `${cwd}/${name}`, isDir: false, ignored };
}

function folder(name: string, ignored = false): FsEntry {
  return { name, path: `${cwd}/${name}`, isDir: true, ignored };
}

function pressPaste(el: HTMLElement) {
  return act(async () => {
    el.dispatchEvent(
      new KeyboardEvent("keydown", { key: "v", metaKey: true, bubbles: true }),
    );
  });
}

function nativeDrop(paths: string[]) {
  return act(async () => {
    dragDrop.handler!({
      payload: { type: "drop", paths, position: { x: 10, y: 10 } },
    });
  });
}

function render(tick = 0, hidden = false) {
  root.render(
    createElement(
      "div",
      { hidden, "data-tick": tick },
      createElement(FileTree, props),
    ),
  );
}

function row(name: string): HTMLButtonElement {
  return container.querySelector(`[role="treeitem"][title="${cwd}/${name}"]`)!;
}

beforeEach(async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  cwd = `/project-${++project}`;
  props = { cwd, onOpenFile: vi.fn() };
  directories.set(cwd, [file("first.ts")]);
  await listCachedDir(cwd);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  localStorage.removeItem("monocode.showExcludedFiles");
  vi.clearAllMocks();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  clipboardFiles.length = 0;
  copied.length = 0;
});

describe("FileTree render isolation", () => {
  it("uses a worktree branch as the explorer root identity", async () => {
    props = { ...props, rootLabel: "mc/update-readme-tests" };

    await act(async () => render());

    const rootButton = container.querySelector<HTMLButtonElement>(
      `button[title="${cwd}"]`,
    )!;
    expect(rootButton.textContent).toContain("mc/update-readme-tests");
    expect(rootButton.lastElementChild?.className).toContain("uppercase");
    expect(
      container.querySelector('[role="tree"]')?.getAttribute("aria-label"),
    ).toBe("mc/update-readme-tests files");
  });

  it.each([false, true])(
    "skips unchanged rows on parent updates (hidden=%s)",
    async (hidden) => {
      await act(async () => render(0, hidden));
      expect(row("first.ts")).not.toBeNull();
      iconRender.mockClear();

      for (let tick = 1; tick <= 20; tick++) act(() => render(tick, hidden));

      expect(iconRender.mock.calls.length).toBe(0);
    },
  );

  it("still updates Git decorations and uses a changed navigation callback", async () => {
    await act(async () => render());
    const onOpenFile = vi.fn();
    props = {
      ...props,
      onOpenFile,
      gitStatuses: {
        files: new Map([[`${cwd}/first.ts`, "modified"]]),
        dirs: new Map(),
      },
    };
    act(() => render(1));
    expect(row("first.ts").querySelector(".text-amber-400")).not.toBeNull();
    act(() => row("first.ts").click());
    expect(onOpenFile).toHaveBeenCalledWith(`${cwd}/first.ts`, undefined, {
      exact: true,
    });
  });

  it("still expands folders and refreshes rows after filesystem changes", async () => {
    saveExpanded(cwd, new Set());
    await act(async () => render());
    expect(row("first.ts")).toBeNull();
    const expand = container.querySelector<HTMLButtonElement>(
      "button[aria-expanded]",
    )!;
    await act(async () => expand.click());
    expect(row("first.ts")).not.toBeNull();

    vi.useFakeTimers();
    directories.set(cwd, [file("added.ts")]);
    await act(async () => {
      notifyDirsChanged();
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(row("added.ts")).not.toBeNull();
    expect(row("first.ts")).toBeNull();
  });
});

describe("FileTree excluded files", () => {
  it("hides ignored entries by default and follows the setting", async () => {
    directories.set(cwd, [
      folder("dist", true),
      folder("src"),
      file("first.ts"),
      file("debug.log", true),
    ]);
    await refreshDir(cwd);
    await act(async () => render());

    expect(row("src")).not.toBeNull();
    expect(row("first.ts")).not.toBeNull();
    expect(row("dist")).toBeNull();
    expect(row("debug.log")).toBeNull();

    act(() => saveShowExcludedFiles(true));
    expect(row("dist")).not.toBeNull();
    expect(row("debug.log")).not.toBeNull();

    act(() => saveShowExcludedFiles(false));
    expect(row("dist")).toBeNull();
    expect(row("debug.log")).toBeNull();
    expect(row("first.ts")).not.toBeNull();
  });
});

describe("FileTree accepts files from outside the tree", () => {
  beforeEach(async () => {
    directories.set(cwd, [folder("docs"), file("first.ts")]);
    directories.set(`${cwd}/docs`, []);
    await refreshDir(cwd);
    await listCachedDir(`${cwd}/docs`);
  });

  it("pastes files from the system clipboard into the selected folder", async () => {
    clipboardFiles.push("/Users/me/Desktop/a.txt", "/Users/me/Desktop/b.txt");
    saveSelected(cwd, `${cwd}/docs`);
    await act(async () => render());
    await pressPaste(row("docs"));
    expect(copied).toEqual([
      { from: "/Users/me/Desktop/a.txt", destParent: `${cwd}/docs` },
      { from: "/Users/me/Desktop/b.txt", destParent: `${cwd}/docs` },
    ]);
  });

  it("pastes into the parent folder when a file is selected", async () => {
    clipboardFiles.push("/Users/me/Desktop/a.txt");
    saveSelected(cwd, `${cwd}/first.ts`);
    await act(async () => render());
    await pressPaste(row("first.ts"));
    expect(copied).toEqual([
      { from: "/Users/me/Desktop/a.txt", destParent: cwd },
    ]);
  });

  it("does nothing on paste when the clipboard holds no files", async () => {
    saveSelected(cwd, `${cwd}/docs`);
    await act(async () => render());
    await pressPaste(row("docs"));
    expect(copied).toEqual([]);
  });

  it("does not read the clipboard when the context menu opens", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    await act(async () => render());
    await act(async () => {
      row("docs").dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
      );
    });
    expect(document.querySelector("[role='menu']")).not.toBeNull();
    expect(vi.mocked(invoke).mock.calls.map((c) => c[0])).not.toContain(
      "clipboard_file_paths",
    );
  });

  it("copies a native file drop into the hovered folder", async () => {
    await act(async () => render());
    expect(dragDrop.handler).not.toBeNull();
    const target = row("docs");
    vi.stubGlobal("devicePixelRatio", 1);
    document.elementFromPoint = () => target;
    await act(async () => {
      dragDrop.handler!({
        payload: { type: "over", position: { x: 10, y: 10 } },
      });
    });
    expect(target.className).toContain("bg-selection");
    await nativeDrop(["/Users/me/Desktop/a.txt"]);
    expect(copied).toEqual([
      { from: "/Users/me/Desktop/a.txt", destParent: `${cwd}/docs` },
    ]);
    expect(target.className).not.toContain("bg-selection");
  });

  it("ignores a native drop outside the tree", async () => {
    await act(async () => render());
    document.elementFromPoint = () => document.body;
    await nativeDrop(["/Users/me/Desktop/a.txt"]);
    expect(copied).toEqual([]);
  });
});

describe("FileTree copies paths", () => {
  it("copies the selected path on Mod+Shift+C", async () => {
    saveSelected(cwd, `${cwd}/first.ts`);
    await act(async () => render());
    await act(async () => {
      row("first.ts").dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "C",
          metaKey: true,
          shiftKey: true,
          bubbles: true,
        }),
      );
    });
    expect(await navigator.clipboard.readText()).toBe(`${cwd}/first.ts`);
  });
});

describe("FileTree starts Explorer file drags", () => {
  beforeEach(async () => {
    directories.set(cwd, [folder("docs"), file("first.ts")]);
    await refreshDir(cwd);
  });

  it("publishes a pointer-driven file drop without opening the file", async () => {
    await act(async () => render());
    const fileRow = row("first.ts");
    const folderRow = row("docs");
    const events: ExplorerFilePointerDragDetail[] = [];
    const onDrag = (event: Event) => {
      events.push((event as CustomEvent<ExplorerFilePointerDragDetail>).detail);
    };
    window.addEventListener(EXPLORER_FILE_POINTER_DRAG_EVENT, onDrag);

    act(() => {
      fileRow.dispatchEvent(
        new PointerEvent("pointerdown", {
          button: 0,
          pointerId: 1,
          clientX: 10,
          clientY: 10,
          bubbles: true,
        }),
      );
      window.dispatchEvent(
        new PointerEvent("pointermove", {
          pointerId: 1,
          clientX: 30,
          clientY: 30,
        }),
      );
    });

    const preview = document.querySelector<HTMLElement>(
      ".explorer-file-drag-preview",
    );
    expect(preview).not.toBeNull();
    expect(preview?.getAttribute("aria-hidden")).toBe("true");
    expect(preview?.textContent).toContain("first.ts");
    expect(preview?.children).toHaveLength(2);
    expect(preview?.querySelectorAll('[data-icon="first.ts"]')).toHaveLength(1);
    expect(preview?.style.transform).toBe("translate3d(18px, 17px, 0)");

    act(() => {
      window.dispatchEvent(
        new PointerEvent("pointermove", {
          pointerId: 1,
          clientX: 36,
          clientY: 38,
        }),
      );
    });
    expect(preview?.style.transform).toBe("translate3d(24px, 25px, 0)");

    act(() => {
      window.dispatchEvent(
        new PointerEvent("pointerup", {
          pointerId: 1,
          clientX: 40,
          clientY: 40,
        }),
      );
      fileRow.click();
    });

    expect(document.querySelector(".explorer-file-drag-preview")).toBeNull();
    expect(events.some((event) => event.type === "move")).toBe(true);
    expect(events.slice(-2)).toEqual([
      {
        type: "drop",
        path: `${cwd}/first.ts`,
        x: 40,
        y: 40,
      },
      { type: "end", path: `${cwd}/first.ts` },
    ]);
    expect(props.onOpenFile).not.toHaveBeenCalled();

    events.length = 0;
    act(() => {
      folderRow.dispatchEvent(
        new PointerEvent("pointerdown", {
          button: 0,
          pointerId: 2,
          clientX: 10,
          clientY: 10,
          bubbles: true,
        }),
      );
      window.dispatchEvent(
        new PointerEvent("pointermove", {
          pointerId: 2,
          clientX: 40,
          clientY: 40,
        }),
      );
      window.dispatchEvent(
        new PointerEvent("pointerup", {
          pointerId: 2,
          clientX: 40,
          clientY: 40,
        }),
      );
    });
    expect(events).toEqual([]);

    window.removeEventListener(EXPLORER_FILE_POINTER_DRAG_EVENT, onDrag);
  });
});

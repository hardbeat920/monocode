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

const {
  iconRender,
  directories,
  clipboardFiles,
  copied,
  moved,
  deleted,
  dragDrop,
  platform,
} = vi.hoisted(() => ({
  iconRender: vi.fn(),
  directories: new Map<string, FsEntry[]>(),
  clipboardFiles: [] as string[],
  copied: [] as { from: string; destParent: string }[],
  moved: [] as string[],
  deleted: [] as string[],
  dragDrop: {
    handler: null as null | ((event: { payload: unknown }) => void),
  },
  platform: { mac: true },
}));

vi.mock("../../../platform/tauri/platform", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../../platform/tauri/platform")>();
  return {
    ...actual,
    get IS_MAC() {
      return platform.mac;
    },
  };
});

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args: Record<string, string>) => {
    if (command === "list_dir") return directories.get(args.path) ?? [];
    if (command === "clipboard_file_paths") return [...clipboardFiles];
    if (command === "copy_path") {
      if (args.from.includes("locked")) throw new Error("Permission denied");
      copied.push({ from: args.from, destParent: args.destParent });
      return `${args.destParent}/${args.from.split("/").pop()}`;
    }
    if (command === "move_path") {
      if (args.from.endsWith("/b.ts")) throw new Error("Permission denied");
      moved.push(args.from);
      return `${args.destParent}/${args.from.split("/").pop()}`;
    }
    if (command === "delete_path") {
      deleted.push(args.path);
      return;
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

function press(el: HTMLElement, init: KeyboardEventInit) {
  return act(async () => {
    el.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, ...init }));
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
  moved.length = 0;
  deleted.length = 0;
  platform.mac = true;
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

  it("keeps pasting past a failed clipboard file and reports it", async () => {
    clipboardFiles.push(
      "/Users/me/Desktop/a.txt",
      "/Users/me/Desktop/locked.txt",
      "/Users/me/Desktop/b.txt",
    );
    saveSelected(cwd, `${cwd}/docs`);
    await act(async () => render());
    await pressPaste(row("docs"));
    expect(copied).toEqual([
      { from: "/Users/me/Desktop/a.txt", destParent: `${cwd}/docs` },
      { from: "/Users/me/Desktop/b.txt", destParent: `${cwd}/docs` },
    ]);
    expect(container.textContent).toContain("Permission denied");
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

  it("pastes on a non-Latin layout", async () => {
    clipboardFiles.push("/Users/me/Desktop/a.txt");
    saveSelected(cwd, `${cwd}/docs`);
    await act(async () => render());
    await press(row("docs"), { key: "м", code: "KeyV", metaKey: true });
    expect(copied).toEqual([
      { from: "/Users/me/Desktop/a.txt", destParent: `${cwd}/docs` },
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
  beforeEach(async () => {
    saveSelected(cwd, `${cwd}/first.ts`);
    await navigator.clipboard.writeText("before");
    await act(async () => render());
  });

  it("copies the selected path on Mod+Shift+C", async () => {
    await press(row("first.ts"), { key: "C", metaKey: true, shiftKey: true });
    expect(await navigator.clipboard.readText()).toBe(`${cwd}/first.ts`);
  });

  it("copies the project root path from the root row", async () => {
    const rootRow = container.querySelector<HTMLButtonElement>(
      "[data-explorer-root]",
    );
    if (!rootRow) throw new Error("Root row not rendered");
    await act(async () => rootRow.click());
    await press(rootRow, { key: "C", metaKey: true, shiftKey: true });
    expect(await navigator.clipboard.readText()).toBe(cwd);
  });

  it("copies the selected path on a non-Latin layout", async () => {
    await press(row("first.ts"), {
      key: "С",
      code: "KeyC",
      metaKey: true,
      shiftKey: true,
    });
    expect(await navigator.clipboard.readText()).toBe(`${cwd}/first.ts`);
  });

  it("matches the typed Latin letter, not the physical key", async () => {
    // Dvorak types "j" on the physical C key.
    await press(row("first.ts"), {
      key: "J",
      code: "KeyC",
      metaKey: true,
      shiftKey: true,
    });
    expect(await navigator.clipboard.readText()).toBe("before");
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
        paths: [`${cwd}/first.ts`],
        x: 40,
        y: 40,
      },
      { type: "end", paths: [`${cwd}/first.ts`] },
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

describe("FileTree multi-selection", () => {
  const entry = (path: string, isDir = false): FsEntry => ({
    name: path.split("/").pop()!,
    path: `${cwd}/${path}`,
    isDir,
    ignored: false,
  });

  beforeEach(async () => {
    directories.set(cwd, [
      entry("docs", true),
      entry("src", true),
      entry("a.ts"),
      entry("b.ts"),
      entry("c.ts"),
    ]);
    directories.set(`${cwd}/docs`, [entry("docs/readme.md")]);
    directories.set(`${cwd}/src`, [entry("src/main.ts")]);
    await refreshDir(cwd);
    await refreshDir(`${cwd}/docs`);
    await refreshDir(`${cwd}/src`);
    saveExpanded(cwd, new Set([cwd, `${cwd}/docs`]));
    await act(async () => render());
  });

  function click(name: string, init: MouseEventInit = {}) {
    return act(async () => {
      row(name).dispatchEvent(
        new MouseEvent("click", { bubbles: true, cancelable: true, ...init }),
      );
    });
  }

  function selected(): string[] {
    return [
      ...container.querySelectorAll<HTMLElement>(
        '[role="treeitem"][aria-selected="true"]',
      ),
    ].map((el) => el.title.slice(cwd.length + 1));
  }

  function isCut(name: string): boolean {
    return row(name).className.split(/\s+/).includes("opacity-50");
  }

  function openMenuOn(name: string) {
    return act(async () => {
      row(name).dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
      );
    });
  }

  function menuItems(): HTMLButtonElement[] {
    return [
      ...document.querySelectorAll<HTMLButtonElement>("[role='menuitem']"),
    ];
  }

  function pick(label: string) {
    const item = menuItems().find((el) => el.textContent?.startsWith(label));
    if (!item) throw new Error(`No menu item ${label}`);
    return act(async () => item.click());
  }

  it("marks the tree as multi-selectable", () => {
    expect(
      container
        .querySelector('[role="tree"]')
        ?.getAttribute("aria-multiselectable"),
    ).toBe("true");
  });

  it("opens files and toggles folders on a plain click, selecting one row", async () => {
    await click("a.ts");
    expect(props.onOpenFile).toHaveBeenCalledWith(`${cwd}/a.ts`, undefined, {
      exact: true,
    });
    expect(selected()).toEqual(["a.ts"]);

    await click("src");
    expect(row("src").getAttribute("aria-expanded")).toBe("true");
    expect(selected()).toEqual(["src"]);
  });

  it("toggles rows on Cmd-click without opening or expanding them", async () => {
    await click("a.ts");
    await click("src", { metaKey: true });
    await click("c.ts", { metaKey: true });
    expect(selected()).toEqual(["src", "a.ts", "c.ts"]);
    expect(row("src").getAttribute("aria-expanded")).toBe("false");
    expect(props.onOpenFile).toHaveBeenCalledTimes(1);

    await click("a.ts", { metaKey: true });
    expect(selected()).toEqual(["src", "c.ts"]);
    expect(props.onOpenFile).toHaveBeenCalledTimes(1);
  });

  it("toggles on Ctrl-click off macOS", async () => {
    platform.mac = false;
    await click("a.ts");
    await click("b.ts", { ctrlKey: true });
    expect(selected()).toEqual(["a.ts", "b.ts"]);
    expect(props.onOpenFile).toHaveBeenCalledTimes(1);

    await click("c.ts", { metaKey: true });
    expect(selected()).toEqual(["c.ts"]);
    expect(props.onOpenFile).toHaveBeenCalledTimes(2);
  });

  it("selects an on-screen range on Shift-click, skipping collapsed children", async () => {
    await click("docs/readme.md");
    await click("b.ts", { shiftKey: true });
    expect(selected()).toEqual(["docs/readme.md", "src", "a.ts", "b.ts"]);
    expect(props.onOpenFile).toHaveBeenCalledTimes(1);

    await click("docs", { shiftKey: true });
    expect(selected()).toEqual(["docs", "docs/readme.md"]);
    expect(row("docs").getAttribute("aria-expanded")).toBe("true");
  });

  it("clears the selection on a root click", async () => {
    await click("a.ts");
    await click("b.ts", { metaKey: true });
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>("[data-explorer-root]")!
        .click(),
    );
    expect(selected()).toEqual([]);
  });

  it("offers bulk items on a selected row and applies them to every row", async () => {
    vi.stubGlobal(
      "confirm",
      vi.fn(() => true),
    );
    await click("a.ts");
    await click("b.ts", { metaKey: true });
    await openMenuOn("b.ts");
    const labels = menuItems().map((el) => el.textContent ?? "");
    expect(labels).toHaveLength(5);
    ["Cut", "Copy", "Copy Path", "Copy Relative Path", "Delete"].forEach(
      (label, i) => expect(labels[i].startsWith(label)).toBe(true),
    );
    expect(selected()).toEqual(["a.ts", "b.ts"]);

    await pick("Copy Relative Path");
    expect(await navigator.clipboard.readText()).toBe("a.ts\nb.ts");

    await openMenuOn("a.ts");
    await pick("Delete");
    expect(window.confirm).toHaveBeenCalledTimes(1);
    expect(window.confirm).toHaveBeenCalledWith("Delete 2 items?");
    expect(deleted).toEqual([`${cwd}/a.ts`, `${cwd}/b.ts`]);
    expect(selected()).toEqual([]);
  });

  it("replaces the selection when right-clicking outside it", async () => {
    await click("a.ts");
    await click("b.ts", { metaKey: true });
    await openMenuOn("c.ts");
    expect(selected()).toEqual(["c.ts"]);
    expect(menuItems().some((el) => el.textContent?.startsWith("Rename"))).toBe(
      true,
    );
  });

  it("clears the selection on a background right-click", async () => {
    await click("a.ts");
    await click("b.ts", { metaKey: true });
    await act(async () => {
      container
        .querySelector('[role="tree"]')!
        .dispatchEvent(
          new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
        );
    });
    expect(selected()).toEqual([]);
  });

  it("deletes only top-level selected paths after one confirm", async () => {
    vi.stubGlobal(
      "confirm",
      vi.fn(() => true),
    );
    await click("docs", { metaKey: true });
    await click("docs/readme.md", { metaKey: true });
    await click("a.ts", { metaKey: true });
    await press(row("a.ts"), { key: "Delete" });
    expect(window.confirm).toHaveBeenCalledTimes(1);
    expect(window.confirm).toHaveBeenCalledWith("Delete 2 items?");
    expect(deleted).toEqual([`${cwd}/docs`, `${cwd}/a.ts`]);
  });

  it("does not delete when the confirm is declined", async () => {
    vi.stubGlobal(
      "confirm",
      vi.fn(() => false),
    );
    await click("a.ts");
    await click("b.ts", { metaKey: true });
    await press(row("b.ts"), { key: "Backspace" });
    expect(deleted).toEqual([]);
    expect(selected()).toEqual(["a.ts", "b.ts"]);
  });

  it("copies every selected file on Copy then Paste", async () => {
    await click("a.ts");
    await click("b.ts", { metaKey: true });
    await press(row("b.ts"), { key: "c", metaKey: true });
    await click("docs");
    await pressPaste(row("docs"));
    expect(copied).toEqual([
      { from: `${cwd}/a.ts`, destParent: `${cwd}/docs` },
      { from: `${cwd}/b.ts`, destParent: `${cwd}/docs` },
    ]);
  });

  it("keeps cut files that failed to move for another Paste", async () => {
    await click("a.ts");
    await click("b.ts", { metaKey: true });
    await press(row("b.ts"), { key: "x", metaKey: true });
    await click("docs");
    await pressPaste(row("docs"));
    expect(moved).toEqual([`${cwd}/a.ts`]);
    expect(container.textContent).toContain("Permission denied");
    expect(isCut("b.ts")).toBe(true);

    await pressPaste(row("docs"));
    expect(moved).toEqual([`${cwd}/a.ts`]);
    expect(isCut("b.ts")).toBe(true);
  });

  it("copies every selected path on Mod+Shift+C, one per line", async () => {
    await click("a.ts");
    await click("src", { metaKey: true });
    await press(row("src"), { key: "C", metaKey: true, shiftKey: true });
    expect(await navigator.clipboard.readText()).toBe(
      `${cwd}/a.ts\n${cwd}/src`,
    );
  });

  it("clears the selection on Escape before clearing the cut", async () => {
    await click("a.ts");
    await click("b.ts", { metaKey: true });
    await press(row("b.ts"), { key: "x", metaKey: true });
    expect(isCut("a.ts")).toBe(true);
    expect(isCut("b.ts")).toBe(true);

    // Only the focused row stays highlighted.
    await press(row("b.ts"), { key: "Escape" });
    expect(selected()).toEqual(["b.ts"]);
    expect(isCut("a.ts")).toBe(true);

    await press(row("b.ts"), { key: "Escape" });
    expect(isCut("a.ts")).toBe(false);
  });

  it("selects every visible row on Mod+A", async () => {
    await press(row("a.ts"), { key: "a", metaKey: true });
    expect(selected()).toEqual([
      "docs",
      "docs/readme.md",
      "src",
      "a.ts",
      "b.ts",
      "c.ts",
    ]);
  });

  it("drops selected rows hidden by collapsing their folder", async () => {
    await click("docs/readme.md", { metaKey: true });
    await click("a.ts", { metaKey: true });
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('button[title="Collapse All"]')!
        .click(),
    );
    expect(selected()).toEqual(["a.ts"]);

    await click("docs", { metaKey: true });
    await click("docs");
    expect(row("docs/readme.md")).not.toBeNull();
    expect(selected()).toEqual(["docs"]);
    await click("a.ts", { metaKey: true });
    expect(selected()).toEqual(["docs", "a.ts"]);
  });

  function drag(name: string) {
    const events: ExplorerFilePointerDragDetail[] = [];
    const onDrag = (event: Event) => {
      events.push((event as CustomEvent<ExplorerFilePointerDragDetail>).detail);
    };
    window.addEventListener(EXPLORER_FILE_POINTER_DRAG_EVENT, onDrag);
    act(() => {
      row(name).dispatchEvent(
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
    const preview = document.querySelector(
      ".explorer-file-drag-preview",
    )?.textContent;
    act(() => {
      window.dispatchEvent(
        new PointerEvent("pointerup", {
          pointerId: 1,
          clientX: 30,
          clientY: 30,
        }),
      );
    });
    window.removeEventListener(EXPLORER_FILE_POINTER_DRAG_EVENT, onDrag);
    return { drop: events.find((e) => e.type === "drop"), preview };
  }

  it("drags every selected file when grabbing a selected row", async () => {
    await click("a.ts");
    await click("src", { metaKey: true });
    await click("c.ts", { metaKey: true });
    const { drop, preview } = drag("c.ts");
    expect(drop).toEqual({
      type: "drop",
      paths: [`${cwd}/a.ts`, `${cwd}/c.ts`],
      x: 30,
      y: 30,
    });
    expect(preview).toContain("+1");
    expect(selected()).toEqual(["src", "a.ts", "c.ts"]);
  });

  it("drags files selected inside a selected folder", async () => {
    await click("docs", { metaKey: true });
    await click("docs/readme.md", { metaKey: true });
    await click("a.ts", { metaKey: true });
    const { drop } = drag("docs/readme.md");
    expect(drop?.type === "drop" && drop.paths).toEqual([
      `${cwd}/docs/readme.md`,
      `${cwd}/a.ts`,
    ]);
  });

  it("drags only the grabbed file when it is outside the selection", async () => {
    await click("a.ts");
    await click("b.ts", { metaKey: true });
    const { drop, preview } = drag("c.ts");
    expect(drop?.type === "drop" && drop.paths).toEqual([`${cwd}/c.ts`]);
    expect(preview).not.toContain("+");
    expect(selected()).toEqual(["c.ts"]);
  });
});

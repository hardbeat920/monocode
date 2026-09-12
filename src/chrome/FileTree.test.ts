// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  listCachedDir,
  notifyDirsChanged,
  saveExpanded,
} from "../lib/fileTree";
import type { FsEntry } from "../lib/fs";
import { FileTree } from "./FileTree";

const { iconRender, directories } = vi.hoisted(() => ({
  iconRender: vi.fn(),
  directories: new Map<string, FsEntry[]>(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args: { path: string }) => {
    if (command !== "list_dir")
      throw new Error(`Unexpected command: ${command}`);
    return directories.get(args.path) ?? [];
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

function file(name: string): FsEntry {
  return { name, path: `${cwd}/${name}`, isDir: false, ignored: false };
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
  vi.clearAllMocks();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("FileTree render isolation", () => {
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

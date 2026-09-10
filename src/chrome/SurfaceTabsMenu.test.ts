// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FilePaneTab } from "../lib/layout";
import { SurfaceTabs } from "./SurfaceTabs";

const actions = vi.hoisted(() => ({
  copyText: vi.fn(async () => {}),
  openPath: vi.fn(async () => {}),
  revealPath: vi.fn(async () => {}),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openPath: actions.openPath,
}));

vi.mock("../lib/clipboard", () => ({
  copyText: actions.copyText,
}));

vi.mock("../lib/fs", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/fs")>()),
  revealPath: actions.revealPath,
}));

vi.mock("./FileTypeIcon", () => ({
  FileTypeIcon: ({ name }: { name: string }) =>
    createElement("span", { "data-icon": name }),
}));

let container: HTMLDivElement;
let root: Root;
let props: ComponentProps<typeof SurfaceTabs>;

const files: FilePaneTab[] = [
  { id: "first", path: "/repo/README.md", cwd: "/repo" },
  { id: "second", path: "/repo/src/app.ts", cwd: "/repo" },
];

function render() {
  act(() => root.render(createElement(SurfaceTabs, props)));
}

function openSecondMenu() {
  const tab = container.querySelector<HTMLButtonElement>(
    '[role="tab"][title="/repo/src/app.ts"]',
  )!;
  act(() => {
    tab.parentElement!.dispatchEvent(
      new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        clientX: 100,
        clientY: 80,
      }),
    );
  });
  return document.querySelector<HTMLElement>(
    '[role="menu"][aria-label="File tab actions"]',
  )!;
}

async function pick(label: string) {
  const menu = openSecondMenu();
  const item = Array.from(
    menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
  ).find((button) => button.textContent === label)!;
  await act(async () => item.click());
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.spyOn(HTMLElement.prototype, "scrollIntoView").mockImplementation(
    () => {},
  );
  props = {
    files,
    activeFileId: "first",
    dirtyFileIds: new Set(),
    fileErrorCounts: new Map(),
    onSelectFile: vi.fn(),
    onCloseFile: vi.fn(),
    onReorder: vi.fn(),
  };
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  render();
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("file tab context menu", () => {
  it("selects the right-clicked tab and exposes its file actions", () => {
    const menu = openSecondMenu();
    expect(props.onSelectFile).toHaveBeenCalledWith("second");
    expect(menu.textContent).toContain("Open in Default App");
    expect(menu.textContent).toContain("Copy Path");
    expect(menu.textContent).toContain("Copy Relative Path");
    expect(menu.textContent).toContain("Copy File Name");
    expect(menu.textContent).toContain("Close");
  });

  it("runs path, external-open, reveal, and close actions for the tab", async () => {
    await pick("Copy Path");
    expect(actions.copyText).toHaveBeenLastCalledWith("/repo/src/app.ts");

    await pick("Copy Relative Path");
    expect(actions.copyText).toHaveBeenLastCalledWith("src/app.ts");

    await pick("Copy File Name");
    expect(actions.copyText).toHaveBeenLastCalledWith("app.ts");

    await pick("Open in Default App");
    expect(actions.openPath).toHaveBeenCalledWith("/repo/src/app.ts");

    const revealLabel = /Mac/.test(navigator.platform)
      ? "Reveal in Finder"
      : /Win/i.test(navigator.platform)
        ? "Reveal in File Explorer"
        : "Open Containing Folder";
    await pick(revealLabel);
    expect(actions.revealPath).toHaveBeenCalledWith("/repo/src/app.ts");

    await pick("Close");
    expect(props.onCloseFile).toHaveBeenCalledWith("second");
  });
});

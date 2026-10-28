// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TitleBar, type Tab } from "./TitleBar";

vi.mock("./WindowControls", () => ({ WindowControls: () => null }));

let container: HTMLDivElement;
let root: Root;

function tab(id: string, sessionCount = 1): Tab {
  return {
    id,
    project: "project",
    title: id,
    more: [],
    sessionCount,
    harnesses: [],
    busyHarnesses: [],
    files: sessionCount ? [] : ["file.ts"],
  };
}

function openMenu(tabId: string): HTMLElement {
  act(() => {
    container
      .querySelector(`[data-title-tab-id="${tabId}"] button`)!
      .dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
      );
  });
  return document.querySelector<HTMLElement>('[role="menu"]')!;
}

function menuItem(menu: HTMLElement, label: string): HTMLButtonElement {
  const item = Array.from(
    menu.querySelectorAll<HTMLButtonElement>("button"),
  ).find((button) => button.textContent?.startsWith(label));
  expect(item).toBeDefined();
  return item!;
}

function pick(menu: HTMLElement, label: string) {
  act(() => menuItem(menu, label).click());
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("title tab context menu", () => {
  it("archives and deletes the right-clicked conversation tab", () => {
    const onArchiveTab = vi.fn();
    const onDeleteTab = vi.fn();
    act(() =>
      root.render(
        createElement(TitleBar, {
          tabs: [tab("active"), tab("other", 2)],
          activeId: "active",
          cwd: "/project",
          onToggleSidebar: vi.fn(),
          onNew: vi.fn(),
          onSelect: vi.fn(),
          onClose: vi.fn(),
          onCloseMany: vi.fn(),
          onArchiveTab,
          onDeleteTab,
          onReorder: vi.fn(),
        }),
      ),
    );

    const archiveMenu = openMenu("other");
    expect(menuItem(archiveMenu, "Archive").textContent).toContain(
      "All 2 conversations in this tab",
    );
    expect(menuItem(archiveMenu, "Archive").textContent).not.toContain(
      "Permanently",
    );
    pick(archiveMenu, "Archive");
    expect(onArchiveTab).toHaveBeenCalledExactlyOnceWith("other");
    expect(onDeleteTab).not.toHaveBeenCalled();

    const deleteMenu = openMenu("other");
    expect(menuItem(deleteMenu, "Delete").textContent).toContain(
      "Permanently delete all 2 conversations in this tab",
    );
    pick(deleteMenu, "Delete");
    expect(onDeleteTab).toHaveBeenCalledExactlyOnceWith("other");
  });

  it("keeps archive and delete off file-only tabs", () => {
    act(() =>
      root.render(
        createElement(TitleBar, {
          tabs: [tab("conversation"), tab("file", 0)],
          activeId: "conversation",
          cwd: "/project",
          onToggleSidebar: vi.fn(),
          onNew: vi.fn(),
          onSelect: vi.fn(),
          onClose: vi.fn(),
          onCloseMany: vi.fn(),
          onArchiveTab: vi.fn(),
          onDeleteTab: vi.fn(),
          onReorder: vi.fn(),
        }),
      ),
    );

    const labels = Array.from(
      openMenu("file").querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
      (item) => item.textContent,
    );
    expect(labels).not.toContain("Archive");
    expect(labels).not.toContain("Delete");
    expect(labels.some((label) => label?.startsWith("Close Tab"))).toBe(true);
  });
});

// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SurfaceTabs } from "./SurfaceTabs";
import { TitleBar, type Tab } from "./TitleBar";

vi.mock("./WindowControls", () => ({ WindowControls: () => null }));

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

function mouse(target: Element, type: string, button: number) {
  const EventClass = type.startsWith("pointer") ? PointerEvent : MouseEvent;
  const event = new EventClass(type, {
    button,
    bubbles: true,
    cancelable: true,
  });
  act(() => target.dispatchEvent(event));
  return event;
}

function click(target: Element, button: number) {
  mouse(target, "pointerdown", button);
  const down = mouse(target, "mousedown", button);
  mouse(target, "pointerup", button);
  mouse(target, "mouseup", button);
  const up = mouse(target, button === 0 ? "click" : "auxclick", button);
  return { down, up };
}

function workspaceTab(id: string): Tab {
  return {
    id,
    project: "project",
    title: id,
    more: [],
    sessionCount: 1,
    harnesses: [],
    busyHarnesses: [],
    files: [],
  };
}

function renderTabs(
  kind: "workspace" | "file" | "terminal",
  activeId = "first",
) {
  const onClose = vi.fn();
  const onSelect = vi.fn();
  const onReorder = vi.fn();
  if (kind === "workspace") {
    act(() =>
      root.render(
        createElement(TitleBar, {
          tabs: [workspaceTab("first"), workspaceTab("second")],
          activeId,
          cwd: "/project",
          onToggleSidebar: vi.fn(),
          onNew: vi.fn(),
          onSelect,
          onClose,
          onCloseMany: vi.fn(),
          onReorder,
        }),
      ),
    );
  } else {
    act(() =>
      root.render(
        createElement(SurfaceTabs, {
          files: ["first", "second"].map((id) => ({
            id,
            path: id,
            cwd: "/project",
            ...(kind === "terminal"
              ? { terminal: true, foreground: "vite" }
              : {}),
          })),
          activeFileId: activeId,
          dirtyFileIds: new Set(kind === "file" ? ["second"] : []),
          fileErrorCounts: new Map(),
          onSelectFile: onSelect,
          onCloseFile: onClose,
          onReorder,
        }),
      ),
    );
  }
  const closeButton = container.querySelector('[aria-label="Close second"]')!;
  const tab = closeButton.parentElement!.querySelector("button")!;
  return { onClose, onSelect, onReorder, tab, closeButton };
}

describe.each(["workspace", "file", "terminal"] as const)(
  "%s tab mouse gestures",
  (kind) => {
    it.each(["first", "second"])(
      "closes on middle release without selecting, active tab is %s",
      (activeId) => {
        const { tab, onClose, onSelect, onReorder } = renderTabs(
          kind,
          activeId,
        );
        const { down, up } = click(tab, 1);
        expect(down.defaultPrevented).toBe(true);
        expect(up.defaultPrevented).toBe(true);
        expect(onClose).toHaveBeenCalledExactlyOnceWith("second");
        expect(onSelect).not.toHaveBeenCalled();
        expect(onReorder).not.toHaveBeenCalled();
      },
    );

    it("does not close on middle press alone", () => {
      const { tab, onClose, onSelect } = renderTabs(kind);
      mouse(tab, "pointerdown", 1);
      mouse(tab, "mousedown", 1);
      expect(onClose).not.toHaveBeenCalled();
      expect(onSelect).not.toHaveBeenCalled();
    });

    it.each(["icon", "padding", "close button"])(
      "accepts middle clicks on the %s",
      (target) => {
        const { tab, closeButton, onClose, onSelect } = renderTabs(kind);
        const element =
          target === "icon"
            ? tab.firstElementChild!
            : target === "padding"
              ? tab.parentElement!
              : closeButton;
        const { down, up } = click(element, 1);
        expect(down.defaultPrevented).toBe(true);
        expect(up.defaultPrevented).toBe(true);
        expect(onClose).toHaveBeenCalledExactlyOnceWith("second");
        expect(onSelect).not.toHaveBeenCalled();
      },
    );

    it("keeps left-click selection and the close button working", () => {
      const { tab, closeButton, onClose, onSelect } = renderTabs(kind);
      click(tab, 0);
      expect(onSelect).toHaveBeenCalledWith("second");
      expect(onClose).not.toHaveBeenCalled();
      onSelect.mockClear();
      click(closeButton, 0);
      expect(onClose).toHaveBeenCalledExactlyOnceWith("second");
      expect(onSelect).not.toHaveBeenCalled();
    });

    it("does not close on right click", () => {
      const { tab, onClose } = renderTabs(kind);
      const { down, up } = click(tab, 2);
      expect(down.defaultPrevented).toBe(false);
      expect(up.defaultPrevented).toBe(false);
      mouse(tab, "contextmenu", 2);
      expect(onClose).not.toHaveBeenCalled();
    });
  },
);

it("keeps the sole blank workspace tab open", () => {
  const onClose = vi.fn();
  const props: ComponentProps<typeof TitleBar> = {
    tabs: [{ ...workspaceTab("blank"), blank: true }],
    activeId: "blank",
    cwd: "/project",
    onToggleSidebar: vi.fn(),
    onNew: vi.fn(),
    onSelect: vi.fn(),
    onClose,
    onCloseMany: vi.fn(),
    onReorder: vi.fn(),
  };
  act(() => root.render(createElement(TitleBar, props)));
  const tab = container.querySelector('[aria-label="project · blank"]')!;
  expect(tab).not.toBeNull();
  click(tab, 1);
  expect(onClose).not.toHaveBeenCalled();
});

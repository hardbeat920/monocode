// @vitest-environment happy-dom
import { act, createElement, useState, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SurfaceTabs } from "./SurfaceTabs";
import { TitleBar, type Tab } from "../../../app/shell/TitleBar";

vi.mock("../../../app/shell/WindowControls", () => ({ WindowControls: () => null }));

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  document.documentElement.style.setProperty(
    "--motion-reorder-duration",
    "160ms",
  );
  document.documentElement.style.setProperty(
    "--motion-ease-out",
    "cubic-bezier(0.22, 1, 0.36, 1)",
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  document.documentElement.removeAttribute("style");
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

function pointer(target: EventTarget, type: string, clientX: number) {
  act(() =>
    target.dispatchEvent(
      new PointerEvent(type, {
        bubbles: true,
        button: 0,
        pointerId: 1,
        clientX,
        clientY: 16,
      }),
    ),
  );
}

function layoutTabs(elements: HTMLElement[]) {
  elements.forEach((element, index) => {
    vi.spyOn(element, "getBoundingClientRect").mockReturnValue(
      new DOMRect(index * 100, 0, 100, 32),
    );
    const captured = new Set<number>();
    element.setPointerCapture = (id) => {
      captured.add(id);
    };
    element.hasPointerCapture = (id) => captured.has(id);
    element.releasePointerCapture = (id) => {
      captured.delete(id);
    };
  });
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
    it("moves neighboring tabs during a drag and saves the order after settling", () => {
      vi.useFakeTimers();
      const { tab, onReorder } = renderTabs(kind);
      const second = tab.parentElement!;
      const first = container.querySelector(
        '[aria-label="Close first"]',
      )!.parentElement!;
      layoutTabs([first, second]);
      try {
        pointer(tab, "pointerdown", 150);
        pointer(window, "pointermove", 30);
        act(() => vi.advanceTimersByTime(32));
        expect(first.style.transform).toBe("translate3d(100px, 0, 0)");
        expect(second.style.transform).toBe("translate3d(-100px, 0, 0)");
        expect(onReorder).not.toHaveBeenCalled();
        pointer(window, "pointerup", 30);
        expect(onReorder).not.toHaveBeenCalled();
        act(() => vi.runAllTimers());
        expect(onReorder).toHaveBeenCalledExactlyOnceWith(
          ["second", "first"],
          "second",
        );
      } finally {
        act(() => window.dispatchEvent(new Event("blur")));
        act(() => vi.runAllTimers());
        vi.useRealTimers();
      }
    });

    it.each([
      ["close button", 60],
      ["middle click", 60],
      ["close button", 160],
      ["middle click", 160],
    ] as const)(
      "preserves the reordered tabs when closing another with %s %i ms after drop",
      (method, delay) => {
        vi.useFakeTimers();
        const onReorder = vi.fn();
        const onClose = vi.fn();
        function Tabs() {
          const [ids, setIds] = useState(["first", "second", "third"]);
          const [activeId, setActiveId] = useState("first");
          const reorder = (next: string[], movedId?: string) => {
            onReorder(next, movedId);
            setIds(next);
          };
          const close = (id: string) => {
            onClose(id);
            setIds((current) => current.filter((entry) => entry !== id));
          };
          return kind === "workspace"
            ? createElement(TitleBar, {
                tabs: ids.map(workspaceTab),
                activeId,
                cwd: "/project",
                onToggleSidebar: vi.fn(),
                onNew: vi.fn(),
                onSelect: setActiveId,
                onClose: close,
                onCloseMany: vi.fn(),
                onReorder: reorder,
              })
            : createElement(SurfaceTabs, {
                files: ids.map((id) => ({
                  id,
                  path: id,
                  cwd: "/project",
                  ...(kind === "terminal"
                    ? { terminal: true, foreground: "vite" }
                    : {}),
                })),
                activeFileId: activeId,
                dirtyFileIds: new Set<string>(),
                fileErrorCounts: new Map<string, number>(),
                onSelectFile: setActiveId,
                onCloseFile: close,
                onReorder: reorder,
              });
        }

        try {
          act(() => root.render(createElement(Tabs)));
          const closeButtons = Array.from(
            container.querySelectorAll<HTMLButtonElement>(
              'button[aria-label^="Close "]',
            ),
          );
          layoutTabs(closeButtons.map((button) => button.parentElement!));
          const second = closeButtons[1].parentElement!.querySelector("button")!;
          pointer(second, "pointerdown", 150);
          pointer(window, "pointermove", 30);
          act(() => vi.advanceTimersByTime(32));
          pointer(window, "pointerup", 30);
          expect(onReorder).not.toHaveBeenCalled();
          act(() => vi.advanceTimersByTime(delay));

          const thirdClose = container.querySelector(
            '[aria-label="Close third"]',
          )!;
          click(
            method === "close button"
              ? thirdClose
              : thirdClose.parentElement!.querySelector("button")!,
            method === "close button" ? 0 : 1,
          );
          expect(onClose).toHaveBeenCalledExactlyOnceWith("third");
          act(() => vi.runAllTimers());

          expect(
            Array.from(
              container.querySelectorAll('button[aria-label^="Close "]'),
              (button) => button.getAttribute("aria-label"),
            ),
          ).toEqual(["Close second", "Close first"]);
          expect(onReorder).toHaveBeenCalledExactlyOnceWith(
            ["second", "first", "third"],
            "second",
          );
          expect(onReorder.mock.invocationCallOrder[0]).toBeLessThan(
            onClose.mock.invocationCallOrder[0],
          );
        } finally {
          act(() => window.dispatchEvent(new Event("blur")));
          act(() => vi.runAllTimers());
          vi.useRealTimers();
        }
      },
    );

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

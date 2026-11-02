// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  newEditorPane,
  newEditorWorkspaceTab,
  newFileTab,
  restorePaneFile,
  restoreWorkspacePaneFile,
  type FilePaneTab,
} from "../../workspace/model/layout";
import {
  attachFileHistoryNavigation,
  useFileHistory,
  visitFile,
} from "./useFileHistory";

const cleanups: (() => void | Promise<void>)[] = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("file history", () => {
  const a = { ...newFileTab("/repo/a.md", "/repo"), preview: true };
  const b = { ...newFileTab("/repo/b.md", "/repo"), preview: true };
  const c = { ...newFileTab("/repo/c.md", "/repo"), preview: true };

  it("drops forward history on a new visit and skips repeated visits", () => {
    let history = visitFile({ entries: [], index: -1 }, a);
    history = visitFile(history, b);
    history = visitFile(history, b);
    expect(history.entries).toEqual([a, b]);
    history = visitFile({ ...history, index: 0 }, c);
    expect(history).toEqual({ entries: [a, c], index: 1 });
  });

  it("uses the explicit fresh ID when a history ID is already in use, retaining pinned files", () => {
    const pinned = { ...newFileTab("/repo/edited.md", "/repo"), id: a.id };
    const pane = {
      ...newEditorPane(b),
      files: [pinned, { ...b, preview: true }],
    };
    const restored = restorePaneFile(pane, a, "restored-a");
    expect(restored.files).toEqual([
      pinned,
      { ...a, id: restored.activeFileId, preview: true },
    ]);
    expect(restored.activeFileId).toBe("restored-a");
    expect(restored.files.filter((file) => file.id === a.id)).toHaveLength(1);
    const existing = restorePaneFile(restored, pinned, "unused-id");
    expect(existing.files).toBe(restored.files);
    expect(existing.activeFileId).toBe(pinned.id);
  });

  it("replaces the existing preview and retains pinned files when restoring from a pinned tab", () => {
    const pinned = newFileTab("/repo/pinned.md", "/repo");
    const pane = { ...newEditorPane(pinned), files: [pinned, b] };
    const restored = restorePaneFile(pane, a, "restored-a");
    expect(restored.files).toEqual([
      pinned,
      { ...a, id: restored.activeFileId, preview: true },
    ]);
    expect(restored.files.some((file) => file.id === b.id)).toBe(false);
  });

  it("ignores an unknown pane and restores only the addressed workspace pane", () => {
    const tab = newEditorWorkspaceTab(b);
    const other = newEditorWorkspaceTab(c);
    const tabs = [tab, other];
    expect(restoreWorkspacePaneFile(tabs, "missing", a, "restored-a")).toBe(
      tabs,
    );
    const restored = restoreWorkspacePaneFile(
      tabs,
      tab.editorPanes[0].id,
      a,
      "restored-a",
    );
    expect(
      restoreWorkspacePaneFile(tabs, tab.editorPanes[0].id, a, "restored-a"),
    ).toEqual(restored);
    expect(restored[1]).toBe(other);
    expect(restored[0].editorPanes[0].files[0].path).toBe(a.path);
    expect(restored[0].focusedId).toBe(tab.editorPanes[0].id);
    expect(restored[0].diffFocused).toBe(false);
    const otherRestored = restoreWorkspacePaneFile(
      tabs,
      other.editorPanes[0].id,
      a,
      "restored-a",
    );
    expect(otherRestored[0]).toBe(tab);
    expect(otherRestored[1].editorPanes[0].files[0].path).toBe(a.path);
  });

  it("creates a preview rather than a permanent tab when no preview exists", () => {
    const pinned = newFileTab("/repo/pinned.md", "/repo");
    const restored = restorePaneFile(newEditorPane(pinned), a, "restored-a");
    expect(restored.files).toEqual([
      pinned,
      { ...a, id: restored.activeFileId, preview: true },
    ]);
  });

  it("keeps history unbounded for the lifetime of the pane", () => {
    let history = { entries: [] as FilePaneTab[], index: -1 };
    for (let index = 0; index < 150; index++) {
      history = visitFile(history, newFileTab(`/repo/${index}.md`, "/repo"));
    }
    expect(history.entries).toHaveLength(150);
    expect(history.entries[0].path).toBe("/repo/0.md");
  });

  function mountHistory() {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    const navigate = vi.fn();
    let time = 0;
    vi.spyOn(performance, "now").mockImplementation(() => time);
    function Harness({
      file,
      visible,
    }: {
      file?: FilePaneTab;
      visible: boolean;
    }) {
      const ref = useFileHistory(file, visible, navigate);
      return createElement("div", { ref }, file?.path);
    }
    cleanups.push(async () => {
      await act(async () => root.unmount());
      container.remove();
    });
    const render = (file?: FilePaneTab, visible = true) =>
      act(async () => root.render(createElement(Harness, { file, visible })));
    const swipe = (deltaX: number) => {
      time += 300;
      const event = new WheelEvent("wheel", {
        deltaX,
        bubbles: true,
        cancelable: true,
      });
      container.firstChild!.dispatchEvent(event);
      return event;
    };
    return { navigate, render, swipe };
  }

  it("goes back and forward without recording replay and drops forward history on a new preview", async () => {
    const view = mountHistory();
    await view.render(a);
    await view.render(b);
    view.swipe(-100);
    expect(view.navigate).toHaveBeenLastCalledWith(a);
    await view.render({ ...a, id: "restored-a" });
    view.swipe(100);
    expect(view.navigate).toHaveBeenLastCalledWith(b);
    await view.render(b);
    view.swipe(-100);
    await view.render(a);
    await view.render(c);
    view.swipe(100);
    expect(view.navigate).toHaveBeenCalledTimes(3);
  });

  it("does not navigate with a closed preview and retains history when a new preview opens", async () => {
    const view = mountHistory();
    await view.render(a);
    await view.render(b);
    await view.render(undefined);
    expect(view.swipe(-100).defaultPrevented).toBe(false);
    expect(view.navigate).not.toHaveBeenCalled();
    await view.render(c);
    view.swipe(-100);
    expect(view.navigate).toHaveBeenCalledExactlyOnceWith(b);
  });

  it("ignores unrelated pinned tabs without truncating forward history", async () => {
    const view = mountHistory();
    await view.render(a);
    await view.render(b);
    view.swipe(-100);
    await view.render(a);
    await view.render({ ...c, preview: false });
    expect(view.swipe(-100).defaultPrevented).toBe(false);
    expect(view.swipe(100).defaultPrevented).toBe(false);
    expect(view.navigate).toHaveBeenCalledTimes(1);
    await view.render(a);
    view.swipe(100);
    expect(view.navigate).toHaveBeenLastCalledWith(b);
  });

  it("can navigate after pinning the current history entry", async () => {
    const view = mountHistory();
    await view.render(a);
    await view.render(b);
    await view.render({ ...b, preview: false });
    view.swipe(-100);
    expect(view.navigate).toHaveBeenCalledExactlyOnceWith(a);
  });

  it("ignores swipes while the pane is hidden", async () => {
    const view = mountHistory();
    await view.render(a);
    await view.render(b, false);
    expect(view.swipe(-100).defaultPrevented).toBe(false);
    expect(view.navigate).not.toHaveBeenCalled();
  });
});

describe("file navigation inputs", () => {
  function setup() {
    const pane = document.createElement("div");
    const content = document.createElement("div");
    pane.append(content);
    document.body.append(pane);
    const navigate = vi.fn(() => true);
    const detach = attachFileHistoryNavigation(pane, navigate);
    cleanups.push(() => {
      detach();
      pane.remove();
    });
    const wheel = (options: WheelEventInit) => {
      const event = new WheelEvent("wheel", {
        bubbles: true,
        cancelable: true,
        ...options,
      });
      // happy-dom's WheelEvent omits the inherited MouseEvent modifiers.
      for (const key of ["ctrlKey", "metaKey", "altKey", "shiftKey"] as const) {
        Object.defineProperty(event, key, { value: options[key] ?? false });
      }
      content.dispatchEvent(event);
      return event;
    };
    return { pane, content, navigate, detach, wheel };
  }

  it("accumulates horizontal motion and navigates once through momentum", () => {
    const view = setup();
    expect(view.wheel({ deltaX: -35 }).defaultPrevented).toBe(false);
    expect(view.wheel({ deltaX: -50 }).defaultPrevented).toBe(true);
    expect(view.wheel({ deltaX: -200 }).defaultPrevented).toBe(true);
    expect(view.navigate).toHaveBeenCalledExactlyOnceWith(-1);
    view.detach();
    view.wheel({ deltaX: 200 });
    expect(view.navigate).toHaveBeenCalledTimes(1);
  });

  it("accepts an opposite swipe while the previous swipe still has momentum", () => {
    const view = setup();
    vi.spyOn(performance, "now").mockReturnValue(100);
    view.wheel({ deltaX: -100 });
    view.wheel({ deltaX: -30 });
    view.wheel({ deltaX: 40 });
    view.wheel({ deltaX: 50 });
    view.wheel({ deltaX: 100 });
    expect(view.navigate.mock.calls).toEqual([[-1], [1]]);
  });

  it.each([-1, 1])(
    "accepts consecutive direction %s swipes after the idle reset",
    (direction) => {
      const view = setup();
      let time = 0;
      vi.spyOn(performance, "now").mockImplementation(() => time);
      for (let gesture = 0; gesture < 3; gesture++) {
        time += 300;
        view.wheel({ deltaX: direction * 100 });
        time += 16;
        view.wheel({ deltaX: direction * 30 });
      }
      expect(view.navigate.mock.calls).toEqual([
        [direction],
        [direction],
        [direction],
      ]);
    },
  );

  it("does not re-arm a consumed swipe after a small direction wobble", () => {
    const view = setup();
    vi.spyOn(performance, "now").mockReturnValue(100);
    view.wheel({ deltaX: -100 });
    view.wheel({ deltaX: 5 });
    view.wheel({ deltaX: -100 });
    expect(view.navigate).toHaveBeenCalledExactlyOnceWith(-1);
  });

  it("leaves vertical motion, zoom and modified scrolling alone", () => {
    const view = setup();
    for (const options of [
      { deltaX: 100, deltaY: 200 },
      { deltaX: 100, ctrlKey: true },
      { deltaX: 100, shiftKey: true },
    ])
      expect(view.wheel(options).defaultPrevented).toBe(false);
    expect(view.navigate).not.toHaveBeenCalled();
    view.detach();
  });

  it.each([0, 200])(
    "preserves horizontal content scrolling at position %s",
    (scrollLeft) => {
      const view = setup();
      Object.defineProperties(view.content, {
        clientWidth: { value: 100 },
        scrollWidth: { value: 300 },
      });
      view.content.style.overflowX = "auto";
      view.content.scrollLeft = scrollLeft;
      expect(view.wheel({ deltaX: 100 }).defaultPrevented).toBe(false);
      expect(view.navigate).not.toHaveBeenCalled();
      view.detach();
    },
  );

  it("normalizes line wheel deltas and honors history boundaries", () => {
    const view = setup();
    view.navigate.mockReturnValue(false);
    expect(view.wheel({ deltaX: 6, deltaMode: 1 }).defaultPrevented).toBe(
      false,
    );
    expect(view.navigate).toHaveBeenCalledExactlyOnceWith(1);
    view.detach();
  });
});

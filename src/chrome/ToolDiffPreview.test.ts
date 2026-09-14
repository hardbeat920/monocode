// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { previewFromTool } from "../lib/harness/claudeProtocol";
import { ToolDiffPreview } from "./ToolDiffPreview";
import { AgentTranscript } from "../surfaces/AgentTranscript";
import type { Block } from "../lib/session";

let container: HTMLDivElement;
let root: Root;
const preview = previewFromTool("Edit", {
  file_path: "/Users/me/Documents/notes.md",
  old_string: "  before\nkeep",
  new_string: "  after\nkeep",
})!;

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function renderPreview() {
  const onOpen = vi.fn();
  const onOpenFile = vi.fn();
  act(() =>
    root.render(
      createElement(ToolDiffPreview, {
        preview,
        label: "notes.md",
        status: "accepted",
        onOpen,
        onOpenFile,
        children: "notes.md",
      }),
    ),
  );
  return { trigger: container.querySelector("button")!, onOpen, onOpenFile };
}

function pointer(
  element: Element,
  type: "pointerover" | "pointerout",
  relatedTarget?: Element,
) {
  act(() =>
    element.dispatchEvent(
      new PointerEvent(type, {
        bubbles: true,
        pointerType: "mouse",
        relatedTarget,
      }),
    ),
  );
}

function advance(ms: number) {
  act(() => vi.advanceTimersByTime(ms));
}

const dialog = () => document.querySelector<HTMLElement>('[role="dialog"]');

describe("tool diff popovers", () => {
  it("labels Write previews accurately, including empty files and failed tools", () => {
    for (const status of ["accepted", "pending", "rejected"] as const) {
      act(() =>
        root.render(
          createElement(ToolDiffPreview, {
            preview: previewFromTool("Write", {
              file_path: "notes.md",
              content: "",
            })!,
            label: "notes.md",
            status,
            children: "notes.md",
          }),
        ),
      );
      act(() => container.querySelector<HTMLButtonElement>("button")!.focus());
      expect(dialog()?.textContent).toContain("Empty file");
      expect(dialog()?.textContent).toContain(
        status === "accepted"
          ? "Written content"
          : status === "pending"
            ? "Proposed changes"
            : "Attempted changes",
      );
      act(() => root.render(null));
    }
  });

  it("waits for hover, stays open across the gap and closes after leaving", () => {
    const { trigger } = renderPreview();
    expect(dialog()).toBeNull();
    pointer(trigger, "pointerover");
    advance(299);
    expect(dialog()).toBeNull();
    advance(1);
    expect(dialog()?.textContent).toContain("before");
    expect(dialog()?.textContent).toContain("after");
    expect(dialog()?.textContent).toContain("+1");
    expect(dialog()?.textContent).toContain("-1");
    pointer(trigger, "pointerout");
    advance(100);
    pointer(dialog()!, "pointerover");
    advance(500);
    expect(dialog()).not.toBeNull();
    pointer(dialog()!, "pointerout");
    advance(200);
    expect(dialog()).toBeNull();
  });

  it("does not flash for a passing pointer and cancels pending work on unmount", () => {
    const { trigger } = renderPreview();
    pointer(trigger, "pointerover");
    advance(100);
    pointer(trigger, "pointerout");
    advance(500);
    expect(dialog()).toBeNull();
    pointer(trigger, "pointerover");
    act(() => root.render(null));
    advance(500);
    expect(dialog()).toBeNull();
  });

  it("preserves the link action, dismisses outside and opens files from the preview", () => {
    const { trigger, onOpen, onOpenFile } = renderPreview();
    pointer(trigger, "pointerover");
    act(() => trigger.click());
    expect(onOpen).toHaveBeenCalledOnce();
    advance(1000);
    expect(dialog()).toBeNull();
    expect(onOpenFile).not.toHaveBeenCalled();
    pointer(trigger, "pointerout");
    pointer(trigger, "pointerover");
    advance(300);
    expect(dialog()).not.toBeNull();
    act(() =>
      document.body.dispatchEvent(
        new PointerEvent("pointerdown", { bubbles: true }),
      ),
    );
    expect(dialog()).toBeNull();
    pointer(trigger, "pointerout");
    pointer(trigger, "pointerover");
    advance(300);
    const fileButton = dialog()!.querySelector<HTMLButtonElement>(
      'button[title="/Users/me/Documents/notes.md"]',
    )!;
    act(() => fileButton.click());
    expect(onOpenFile).toHaveBeenCalledWith("/Users/me/Documents/notes.md");
    expect(dialog()).toBeNull();
  });

  it("opens on focus, supports entering the preview, and restores focus on Escape", () => {
    const { trigger } = renderPreview();
    act(() => trigger.focus());
    expect(dialog()).not.toBeNull();
    expect(trigger.getAttribute("aria-expanded")).toBe("true");
    act(() =>
      trigger.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Tab",
          bubbles: true,
          cancelable: true,
        }),
      ),
    );
    expect(dialog()?.contains(document.activeElement)).toBe(true);
    act(() =>
      document.activeElement!.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      ),
    );
    expect(dialog()).toBeNull();
    expect(document.activeElement).toBe(trigger);
    advance(1000);
    expect(dialog()).toBeNull();
  });

  it.each([true, false])(
    "previews expanded activity and preserves file navigation (diff handler: %s)",
    (hasDiffHandler) => {
      const onOpenFile = vi.fn();
      const onOpenDiff = hasDiffHandler ? vi.fn() : undefined;
      const blocks: Block[] = [
        { id: "user", role: "user", text: "Update my notes" },
        {
          id: "edit",
          role: "tool",
          text: "Edit /Users/me/Documents/notes.md",
          tool: { kind: "edit", status: "completed", preview },
        },
        { id: "answer", role: "assistant", text: "Updated your notes." },
      ];
      act(() =>
        root.render(
          createElement(AgentTranscript, { blocks, onOpenFile, onOpenDiff }),
        ),
      );
      expect(container.textContent).not.toContain("before");
      const expand = container.querySelector<HTMLButtonElement>(
        'button[aria-label="Show the work"]',
      )!;
      act(() => expand.click());
      const trigger = container.querySelector<HTMLButtonElement>(
        'button[aria-haspopup="dialog"]',
      )!;
      expect(trigger).not.toBeNull();
      act(() => trigger.focus());
      expect(dialog()?.textContent).toContain("before");
      expect(dialog()?.textContent).toContain("after");
      act(() => trigger.click());
      expect(onOpenDiff ?? onOpenFile).toHaveBeenCalledWith(
        "/Users/me/Documents/notes.md",
      );
      if (onOpenDiff) expect(onOpenFile).not.toHaveBeenCalled();
      expect(dialog()).toBeNull();
    },
  );
});

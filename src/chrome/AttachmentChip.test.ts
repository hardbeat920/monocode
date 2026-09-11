// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Attachment } from "../lib/session";
import { AttachmentChip } from "./AttachmentChip";

const attachment: Attachment = {
  id: "image-1",
  name: "diagram.png",
  mimeType: "image/png",
  kind: "image",
  size: 42,
  previewUrl: "blob:image-preview",
};

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

function render(onRemove?: () => void) {
  act(() =>
    root.render(createElement(AttachmentChip, { attachment, onRemove })),
  );
}

describe("AttachmentChip image preview", () => {
  it("opens the image full screen and closes it with Escape", () => {
    render();
    const trigger = container.querySelector<HTMLButtonElement>(
      '[aria-label="Open diagram.png full screen"]',
    )!;

    act(() => trigger.focus());
    act(() => trigger.click());

    const dialog = document.querySelector<HTMLElement>('[role="dialog"]');
    expect(dialog?.getAttribute("aria-label")).toBe(
      "Image preview: diagram.png",
    );
    expect(dialog?.querySelector("img")?.getAttribute("src")).toBe(
      attachment.previewUrl,
    );
    expect(document.activeElement?.getAttribute("aria-label")).toBe(
      "Close image preview",
    );

    act(() =>
      window.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      ),
    );

    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it("removes an image without opening the preview", () => {
    const onRemove = vi.fn();
    render(onRemove);

    act(() =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Remove diagram.png"]')!
        .click(),
    );

    expect(onRemove).toHaveBeenCalledOnce();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });
});

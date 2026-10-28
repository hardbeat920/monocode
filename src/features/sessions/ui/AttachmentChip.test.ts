// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Attachment, GeneratedImageMeta } from "../model/session";
import * as fs from "../../../platform/tauri/fs";
import { GeneratedImage } from "./GeneratedImage";

vi.mock("../../../platform/tauri/fs", () => ({
  readBinaryFile: vi.fn(),
}));
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
  vi.mocked(fs.readBinaryFile).mockReset();
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
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

  it("loads persisted generated images and opens a lightbox", async () => {
    const image: GeneratedImageMeta = {
      path: "/app-data/generated-images/image.png",
      name: "generated-image",
      mimeType: "image/png",
      size: 8,
      alt: "A clean product photo",
    };
    vi.mocked(fs.readBinaryFile).mockResolvedValue(
      new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    );
    vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:generated-image");
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);

    await act(async () => {
      root.render(createElement(GeneratedImage, { image }));
      await Promise.resolve();
      await Promise.resolve();
    });

    const preview = container.querySelector<HTMLImageElement>("img");
    expect(preview?.getAttribute("src")).toBe("blob:generated-image");
    expect(preview?.getAttribute("alt")).toBe("A clean product photo");

    const trigger = container.querySelector<HTMLButtonElement>(
      '[aria-label="Open generated-image full screen"]',
    )!;
    act(() => trigger.click());
    expect(document.querySelector('[role="dialog"] img')?.getAttribute("src")).toBe(
      "blob:generated-image",
    );
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

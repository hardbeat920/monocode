// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Attachment, GeneratedImageMeta } from "../model/session";
import * as fs from "../../../platform/tauri/fs";
import { GeneratedImage } from "./GeneratedImage";
import { downloadFile } from "../../../platform/tauri/download";

vi.mock("../../../platform/tauri/download", () => ({
  downloadFile: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../../platform/tauri/fs", () => ({
  readBinaryFile: vi.fn(),
  basename: (path: string) => path.split("/").pop(),
}));
vi.mock("../../../platform/tauri/trackpadZoom", () => ({
  claimTrackpadMagnify: () => () => {},
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
  vi.mocked(downloadFile).mockReset().mockResolvedValue(undefined);
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
    await act(async () => {
      container
        .querySelector<HTMLButtonElement>(
          '[aria-label="Download generated-image.png"]',
        )!
        .click();
    });
    expect(downloadFile).toHaveBeenCalledWith("generated-image.png", {
      path: image.path,
    });
    expect(document.querySelector('[role="dialog"]')).toBeNull();

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
    expect(
      container.querySelector('[aria-label="Download diagram.png"]'),
    ).toBeNull();
  });

  it("downloads sent files of other types", async () => {
    await act(async () =>
      root.render(
        createElement(AttachmentChip, {
          attachment: {
            ...attachment,
            name: "report.pdf",
            kind: "file",
            mimeType: "application/pdf",
            path: "/workspace/report.pdf",
            previewUrl: undefined,
          },
        }),
      ),
    );
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Download report.pdf"]')!
        .click(),
    );
    expect(downloadFile).toHaveBeenCalledWith("report.pdf", {
      path: "/workspace/report.pdf",
    });
  });

  it("keeps folders out of the file download flow", () => {
    act(() =>
      root.render(
        createElement(AttachmentChip, {
          attachment: {
            ...attachment,
            kind: "file",
            mimeType: "inode/directory",
            name: "docs",
            path: "/repo/docs",
            previewUrl: undefined,
          },
        }),
      ),
    );
    expect(container.textContent).toContain("docs");
    expect(container.querySelector('[aria-label^="Download "]')).toBeNull();
  });
});

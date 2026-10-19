// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const readProjectImage = vi.hoisted(() => vi.fn());

vi.mock("../../../platform/tauri/fs", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../platform/tauri/fs")>()),
  readProjectImage,
}));

import { DocumentImage } from "./DocumentImage";

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
const text = (value: string) => new TextEncoder().encode(value);

describe("DocumentImage", () => {
  let container: HTMLDivElement;
  let root: Root;
  let blobs: Blob[];
  const createObjectURL = vi.fn((blob: Blob) => {
    blobs.push(blob);
    return `blob:test/${blobs.length}`;
  });
  const revokeObjectURL = vi.fn();

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    // Without an observer the image loads straight away.
    vi.stubGlobal("IntersectionObserver", undefined);
    URL.createObjectURL = createObjectURL;
    URL.revokeObjectURL = revokeObjectURL;
    blobs = [];
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  async function show(relative: string, attributes = {}) {
    await act(async () => {
      root.render(
        createElement(DocumentImage, {
          path: `/repo/${relative}`,
          root: "/repo",
          relative,
          alt: "Hero",
          ...attributes,
        }),
      );
    });
  }

  it("shows a project image through a blob URL with its attributes", async () => {
    readProjectImage.mockResolvedValue(PNG);
    await show(".github/readme/hero.png", {
      title: "MonoCode",
      width: "600",
      align: "right",
    });

    expect(readProjectImage).toHaveBeenCalledWith(
      "/repo",
      ".github/readme/hero.png",
    );
    const image = container.querySelector("img")!;
    expect(image.getAttribute("src")).toBe("blob:test/1");
    expect(image.getAttribute("alt")).toBe("Hero");
    expect(image.getAttribute("title")).toBe("MonoCode");
    expect(image.getAttribute("width")).toBe("600");
    expect(image.getAttribute("align")).toBe("right");
    expect(image.style.maxWidth).toBe("100%");
    expect(blobs[0].type).toBe("image/png");

    act(() => root.unmount());
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:test/1");
    root = createRoot(container);
  });

  it("frees the blob URL and shows alt text when the image fails to decode", async () => {
    readProjectImage.mockResolvedValue(PNG);
    await show("broken.png");

    const image = container.querySelector("img")!;
    act(() => {
      image.dispatchEvent(new Event("error"));
    });
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector("[data-image-alt]")?.textContent).toBe(
      "Hero",
    );
    expect(revokeObjectURL).toHaveBeenCalledTimes(1);
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:test/1");

    act(() => root.unmount());
    expect(revokeObjectURL).toHaveBeenCalledTimes(1);
    root = createRoot(container);
  });

  it("shows an SVG as an image", async () => {
    readProjectImage.mockResolvedValue(
      text('<svg xmlns="http://www.w3.org/2000/svg"><script>x()</script></svg>'),
    );
    await show("logo.svg");

    expect(container.querySelector("img")?.getAttribute("src")).toBe(
      "blob:test/1",
    );
    expect(blobs[0].type).toBe("image/svg+xml");
  });

  it.each([
    [
      "a Git LFS pointer",
      text("version https://git-lfs.github.com/spec/v1\noid sha256:abc\nsize 9\n"),
    ],
    ["HTML named as an image", text("<html><script>x()</script></html>")],
  ])("shows alt text for %s", async (_label, bytes) => {
    readProjectImage.mockResolvedValue(bytes);
    await show("hero.png");

    expect(container.querySelector("img")).toBeNull();
    expect(createObjectURL).not.toHaveBeenCalled();
    expect(container.querySelector("[data-image-alt]")?.textContent).toBe(
      "Hero",
    );
  });

  it("shows alt text when the file cannot be read", async () => {
    readProjectImage.mockRejectedValue(new Error("Image is outside the project"));
    await show("leak.png");

    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector("[data-image-alt]")?.textContent).toBe(
      "Hero",
    );
  });
});

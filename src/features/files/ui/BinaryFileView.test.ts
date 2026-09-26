// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BinaryFileView } from "./BinaryFileView";

const actions = vi.hoisted(() => ({
  copyFileToClipboard: vi.fn(async () => {}),
  readBinaryFile: vi.fn(
    async () =>
      new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  ),
}));

vi.mock("../../../platform/tauri/fs", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../platform/tauri/fs")>()),
  copyFileToClipboard: actions.copyFileToClipboard,
  readBinaryFile: actions.readBinaryFile,
}));

vi.mock("../model/fileWatch", () => ({
  watchFile: () => () => {},
}));

vi.mock("./PdfView", () => ({
  PdfView: ({ size }: { size: number }) =>
    createElement("div", { "data-testid": "pdf-view" }, `pdf ${size}`),
}));

vi.mock("../../../platform/tauri/platform", () => ({
  IS_MAC: true,
  IS_WIN: false,
}));

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:image-preview");
  vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
  vi.clearAllMocks();
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

async function renderViewer() {
  await act(async () => {
    root.render(
      createElement(BinaryFileView, {
        path: "/repo/art/original image.png",
        cwd: "/repo",
      }),
    );
    await Promise.resolve();
  });
}

describe("BinaryFileView image copy", () => {
  it("replaces the webview image menu with an original-file copy action", async () => {
    await renderViewer();
    const image = container.querySelector("img")!;
    const event = new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: 80,
      clientY: 60,
    });

    act(() => image.dispatchEvent(event));

    expect(event.defaultPrevented).toBe(true);
    const menu = document.querySelector<HTMLElement>(
      '[role="menu"][aria-label="Image actions"]',
    )!;
    const copy = Array.from(
      menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
    ).find((button) => button.textContent === "Copy Original File")!;

    await act(async () => copy.click());

    expect(actions.copyFileToClipboard).toHaveBeenCalledWith(
      "/repo/art/original image.png",
    );
  });

  it("copies the original file from the viewer footer", async () => {
    await renderViewer();

    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Copy original file"]')!
        .click(),
    );

    expect(actions.copyFileToClipboard).toHaveBeenCalledWith(
      "/repo/art/original image.png",
    );
    expect(container.querySelector('[aria-label="Copied"]')).not.toBeNull();
  });
});

describe("BinaryFileView routing", () => {
  it("hands PDF bytes to the PDF viewer", async () => {
    const pdf = new TextEncoder().encode("%PDF-1.7\n%EOF");
    actions.readBinaryFile.mockResolvedValueOnce(pdf);

    await act(async () => {
      root.render(
        createElement(BinaryFileView, { path: "/repo/spec.pdf", cwd: "/repo" }),
      );
      await Promise.resolve();
    });

    expect(
      container.querySelector('[data-testid="pdf-view"]')?.textContent,
    ).toBe(`pdf ${pdf.byteLength}`);
    expect(URL.createObjectURL).not.toHaveBeenCalled();
  });

  it("shows the unsupported card for bytes that are neither image nor PDF", async () => {
    actions.readBinaryFile.mockResolvedValueOnce(
      new TextEncoder().encode("<html></html>"),
    );

    await act(async () => {
      root.render(
        createElement(BinaryFileView, { path: "/repo/fake.pdf", cwd: "/repo" }),
      );
      await Promise.resolve();
    });

    expect(container.textContent).toContain("not a readable image or PDF");
  });
});

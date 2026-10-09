// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PdfView } from "./PdfView";

const pdf = vi.hoisted(() => ({
  openPdfDocument: vi.fn(),
}));

vi.mock("../model/pdfDocument", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../model/pdfDocument")>()),
  openPdfDocument: pdf.openPdfDocument,
}));

type FakePage = {
  getViewport: (options: { scale: number }) => { width: number; height: number };
  render: () => { promise: Promise<void>; cancel: () => void };
};

function fakePage(render: () => Promise<void> = async () => {}): FakePage {
  return {
    getViewport: ({ scale }) => ({ width: 612 * scale, height: 792 * scale }),
    render: () => ({ promise: render(), cancel: () => {} }),
  };
}

function fakeDocument(
  numPages: number,
  getPage: (number: number) => Promise<FakePage>,
) {
  const destroy = vi.fn();
  pdf.openPdfDocument.mockReturnValue({
    promise: Promise.resolve({ numPages, getPage }),
    destroy,
  });
  return { destroy };
}

// One array for every render, as BinaryFileView keeps its bytes between
// renders. New bytes mean the file changed and correctly reopen the document.
const bytes = new Uint8Array([1, 2, 3]);

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  // Report every page as near the viewport so it renders.
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      constructor(
        private callback: (entries: { isIntersecting: boolean }[]) => void,
      ) {}
      observe() {
        this.callback([{ isIntersecting: true }]);
      }
      disconnect() {}
    },
  );
  pdf.openPdfDocument.mockReset();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

async function render(visible: boolean) {
  await act(async () => {
    root.render(
      createElement(PdfView, {
        bytes,
        size: 3,
        visible,
        onError: () => {},
      }),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

/** Wait past the page's render delay and let its promises settle. */
async function settleRenders() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 50));
  });
}

describe("PdfView", () => {
  it("waits for its tab to show before opening the document", async () => {
    const { destroy } = fakeDocument(1, async () => fakePage());

    await render(false);
    expect(pdf.openPdfDocument).not.toHaveBeenCalled();

    await render(true);
    expect(pdf.openPdfDocument).toHaveBeenCalledTimes(1);

    // Hiding the tab again keeps the document open.
    await render(false);
    expect(pdf.openPdfDocument).toHaveBeenCalledTimes(1);
    expect(destroy).not.toHaveBeenCalled();
  });

  it("shows the document before later pages are measured", async () => {
    fakeDocument(120, (number) =>
      number === 1 ? Promise.resolve(fakePage()) : new Promise(() => {}),
    );

    await render(true);

    expect(container.textContent).toContain("Page 1 of 120");
    expect(container.querySelectorAll("[data-page]")).toHaveLength(120);
  });

  it("reports a page that fails to draw on that page", async () => {
    fakeDocument(1, async () =>
      fakePage(() => Promise.reject(new Error("bad operator"))),
    );

    await render(true);
    await settleRenders();

    expect(container.querySelector('[role="alert"]')?.textContent).toBe(
      "Couldn’t draw page 1: bad operator",
    );
  });

  it("stays quiet when a render is cancelled", async () => {
    const cancelled = Object.assign(new Error("cancelled"), {
      name: "RenderingCancelledException",
    });
    fakeDocument(1, async () => fakePage(() => Promise.reject(cancelled)));

    await render(true);
    await settleRenders();

    expect(container.querySelector('[role="alert"]')).toBeNull();
  });
});

// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PdfViewer } from "./PdfViewer";

type Link = { subtype: string; url: string; rect: number[] };
const mock = vi.hoisted(() => ({
  open: vi.fn(async (_url: string) => {}),
  getDocument: vi.fn(),
  deferFirst: false,
  firstRequested: false,
  resolveFirst: null as ((links: Link[]) => void) | null,
}));
vi.mock("pdfjs-dist/legacy/build/pdf.mjs", () => ({
  GlobalWorkerOptions: { workerSrc: "" },
  getDocument: mock.getDocument,
}));
vi.mock("pdfjs-dist/legacy/build/pdf.worker.min.mjs?url", () => ({ default: "/worker.mjs" }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: mock.open }));
vi.mock("../model/fileWatch", () => ({ watchFile: () => () => {} }));
vi.mock("../../../platform/tauri/fs", async (original) => ({
  ...(await original<typeof import("../../../platform/tauri/fs")>()),
  readBinaryFile: vi.fn(async () => new Uint8Array([37, 80, 68, 70, 45, 49, 46, 55])),
}));

const safe = (page: number): Link => ({
  subtype: "Link", url: `https://example.test/page-${page}`, rect: [10, 10, 100, 40],
});
describe("PDF link and render lifecycle", () => {
  let root: Root;
  let container: HTMLDivElement;
  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("DOMMatrix", class {
      constructor(private values: number[]) {}
      transformPoint({ x, y }: { x: number; y: number }) {
        const [a, b, c, d, e, f] = this.values;
        return { x: a * x + c * y + e, y: b * x + d * y + f };
      }
    });
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(() => ({} as CanvasRenderingContext2D));
    mock.open.mockReset().mockResolvedValue(undefined);
    mock.deferFirst = false;
    mock.firstRequested = false;
    mock.resolveFirst = null;
    mock.getDocument.mockReset().mockImplementation(() => ({
      promise: Promise.resolve({
        numPages: 2,
        getPage: async (page: number) => ({
          getViewport: ({ scale }: { scale: number }) => ({
            width: 300 * scale, height: 400 * scale, transform: [scale, 0, 0, -scale, 0, 400 * scale],
          }),
          render: () => ({ promise: Promise.resolve(), cancel: vi.fn() }),
          getAnnotations: () => {
            if (page === 1 && mock.deferFirst && !mock.firstRequested) {
              mock.firstRequested = true;
              return new Promise<Link[]>((resolve) => { mock.resolveFirst = resolve; });
            }
            return Promise.resolve([safe(page), { ...safe(page), url: "javascript:alert(1)" }]);
          },
        }),
      }),
      destroy: vi.fn(async () => {}),
    }));
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });
  async function render() {
    await act(async () => root.render(createElement(PdfViewer, { path: "D:/document.pdf", cwd: "D:/" })));
  }
  async function clickLink() {
    await act(async () => container.querySelector("a")!.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  }
  it("routes HTTP links through the URL opener and excludes executable URLs", async () => {
    await render();
    await vi.waitFor(() => expect(container.querySelectorAll("a")).toHaveLength(1));
    await clickLink();
    expect(mock.open).toHaveBeenCalledWith("https://example.test/page-1");
  });
  it("preserves the canvas and zoom after an external-opening failure", async () => {
    await render();
    await vi.waitFor(() => expect(container.querySelector("a")).not.toBeNull());
    mock.open.mockRejectedValueOnce(new Error("Browser unavailable"));
    await clickLink();
    expect(container.querySelector('[role="alert"]')!.textContent).toBe("Browser unavailable");
    const canvas = container.querySelector("canvas")!;
    expect(canvas).not.toBeNull();
    const zoom = container.querySelector('[aria-label="Zoom in"]')!;
    await act(async () => zoom.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    await vi.waitFor(() => expect(canvas.width).toBeGreaterThan(300));
    await clickLink();
    expect(container.querySelector('[role="alert"]')).toBeNull();
  });
  it("does not append old annotation links after changing pages", async () => {
    mock.deferFirst = true;
    await render();
    await vi.waitFor(() => expect(mock.resolveFirst).not.toBeNull());
    await act(async () => container.querySelector('[aria-label="Next page"]')!.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    await vi.waitFor(() => expect(container.querySelector("a")?.getAttribute("href")).toBe("https://example.test/page-2"));
    await act(async () => { mock.resolveFirst!([safe(1)]); });
    expect([...container.querySelectorAll("a")].map((link) => link.getAttribute("href"))).toEqual(["https://example.test/page-2"]);
  });
});

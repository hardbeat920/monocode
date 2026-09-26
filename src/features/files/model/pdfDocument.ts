import type {
  PDFDocumentLoadingTask,
  PDFDocumentProxy,
} from "pdfjs-dist/legacy/build/pdf.mjs";

export type { PDFDocumentProxy, PDFPageProxy } from "pdfjs-dist/legacy/build/pdf.mjs";

/** Where the Vite `pdfjs-assets` plugin serves the files pdf.js fetches. */
const ASSET_BASE = `${import.meta.env.BASE_URL}pdfjs/`;

let pdfjs: Promise<typeof import("pdfjs-dist/legacy/build/pdf.mjs")> | null =
  null;

/**
 * Load pdf.js on first use so the library and its worker stay out of the
 * startup bundle. The legacy build carries polyfills for the WebKit versions
 * Tauri runs on macOS and Linux.
 */
function loadPdfjs() {
  pdfjs ??= Promise.all([
    import("pdfjs-dist/legacy/build/pdf.mjs"),
    import("pdfjs-dist/legacy/build/pdf.worker.min.mjs?url"),
  ]).then(
    ([lib, worker]) => {
      lib.GlobalWorkerOptions.workerSrc = worker.default;
      return lib;
    },
    (cause: unknown) => {
      // Forget the failure so the viewer's Retry imports again.
      pdfjs = null;
      throw cause;
    },
  );
  return pdfjs;
}

/**
 * Canvas limits that hold across the WebKit and WebView2 versions Tauri runs
 * on. A canvas past either limit fails to draw, and one at the area limit
 * already holds 64 MB of pixels.
 */
export const MAX_CANVAS_AREA = 4096 * 4096;
export const MAX_CANVAS_SIDE = 8192;

/**
 * Device pixels per CSS pixel for a page drawn `width` by `height` CSS pixels.
 * It is the display's ratio unless that would push the bitmap past a canvas
 * limit, in which case the page draws at lower resolution and the browser
 * scales it up to the same size on screen.
 */
export function renderPixelRatio(
  width: number,
  height: number,
  devicePixelRatio: number,
): number {
  if (width <= 0 || height <= 0) return devicePixelRatio;
  return Math.min(
    devicePixelRatio,
    Math.sqrt(MAX_CANVAS_AREA / (width * height)),
    MAX_CANVAS_SIDE / width,
    MAX_CANVAS_SIDE / height,
  );
}

export type PdfHandle = {
  promise: Promise<PDFDocumentProxy>;
  /** Stop loading, or free the worker and memory of a loaded document. */
  destroy: () => void;
};

/**
 * Parse PDF bytes into a document. pdf.js transfers the buffer to its worker,
 * so callers must not reuse `bytes` afterwards.
 */
export function openPdfDocument(bytes: Uint8Array): PdfHandle {
  let destroyed = false;
  let task: PDFDocumentLoadingTask | null = null;
  const promise = loadPdfjs().then((lib) => {
    if (destroyed) throw new Error("PDF loading was cancelled.");
    task = lib.getDocument({
      data: bytes,
      cMapUrl: `${ASSET_BASE}cmaps/`,
      cMapPacked: true,
      standardFontDataUrl: `${ASSET_BASE}standard_fonts/`,
      wasmUrl: `${ASSET_BASE}wasm/`,
      // The CSP forbids compiling wasm, so pdf.js would only log errors trying.
      useWasm: false,
      // Scripts and forms are out of scope for a read-only preview.
      enableXfa: false,
    });
    return task.promise;
  });
  return {
    promise,
    destroy: () => {
      destroyed = true;
      void task?.destroy();
    },
  };
}

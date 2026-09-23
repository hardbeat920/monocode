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
  ]).then(([lib, worker]) => {
    lib.GlobalWorkerOptions.workerSrc = worker.default;
    return lib;
  });
  return pdfjs;
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

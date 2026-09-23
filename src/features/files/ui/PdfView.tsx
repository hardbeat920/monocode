import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Minus, Plus } from "../../../shared/ui/icons";
import { formatFileSize } from "../model/filePreview";
import {
  openPdfDocument,
  type PDFDocumentProxy,
} from "../model/pdfDocument";
import { clampZoom, ZoomButton } from "./ViewerControls";

/** Space between pages and around the page column, in CSS pixels. */
const PAGE_GAP = 16;

/** Pages this far outside the viewport render early so scrolling stays smooth. */
const RENDER_MARGIN = "100% 0px";

type Size = { width: number; height: number };

type Props = {
  bytes: Uint8Array;
  size: number;
  onError: (message: string) => void;
};

/**
 * Scrolling column of PDF pages. Every page gets a placeholder of its real
 * size up front, so the scrollbar is right from the start, and a page draws to
 * its canvas only when it comes near the viewport.
 */
export function PdfView({ bytes, size, onError }: Props) {
  const [doc, setDoc] = useState<PDFDocumentProxy | null>(null);
  const [pageSizes, setPageSizes] = useState<Size[]>([]);
  const [zoom, setZoom] = useState<number | "fit">("fit");
  const [viewportWidth, setViewportWidth] = useState(0);
  const [currentPage, setCurrentPage] = useState(1);
  const scroller = useRef<HTMLDivElement>(null);
  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;

  useEffect(() => {
    let cancelled = false;
    setDoc(null);
    setPageSizes([]);

    // pdf.js transfers the buffer it is given to the worker. A copy keeps the
    // caller's bytes usable if this effect runs again.
    const handle = openPdfDocument(bytes.slice());
    handle.promise
      .then(async (loaded) => {
        if (cancelled) return;
        const sizes: Size[] = [];
        for (let number = 1; number <= loaded.numPages; number += 1) {
          const page = await loaded.getPage(number);
          const { width, height } = page.getViewport({ scale: 1 });
          sizes.push({ width, height });
        }
        if (cancelled) return;
        setPageSizes(sizes);
        setDoc(loaded);
      })
      .catch((cause: unknown) => {
        if (cancelled) return;
        onErrorRef.current(describePdfError(cause));
      });

    return () => {
      cancelled = true;
      handle.destroy();
    };
  }, [bytes]);

  useLayoutEffect(() => {
    const element = scroller.current;
    if (!element) return;
    const observer = new ResizeObserver(() =>
      setViewportWidth(element.clientWidth),
    );
    observer.observe(element);
    setViewportWidth(element.clientWidth);
    return () => observer.disconnect();
  }, []);

  const widest = pageSizes.reduce((max, page) => Math.max(max, page.width), 0);
  const fitScale =
    widest > 0 && viewportWidth > 0
      ? clampZoom((viewportWidth - PAGE_GAP * 2) / widest)
      : 1;
  const scale = zoom === "fit" ? fitScale : zoom;

  const onScroll = useCallback(() => {
    const element = scroller.current;
    if (!element) return;
    // The page whose top edge last crossed the middle of the viewport.
    const middle = element.scrollTop + element.clientHeight / 2;
    let top = PAGE_GAP;
    let page = 1;
    for (let index = 0; index < pageSizes.length; index += 1) {
      if (top > middle) break;
      page = index + 1;
      top += pageSizes[index].height * scale + PAGE_GAP;
    }
    setCurrentPage(page);
  }, [pageSizes, scale]);

  useEffect(onScroll, [onScroll]);

  const stepZoom = (factor: number) =>
    setZoom((value) => clampZoom((value === "fit" ? fitScale : value) * factor));

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div
        ref={scroller}
        onScroll={onScroll}
        className="min-h-0 flex-1 overflow-auto overscroll-contain bg-content/5"
      >
        {doc ? (
          <div
            className="flex w-max min-w-full flex-col items-center"
            style={{ gap: PAGE_GAP, padding: PAGE_GAP }}
          >
            {pageSizes.map((pageSize, index) => (
              <PdfPage
                key={index}
                doc={doc}
                number={index + 1}
                width={pageSize.width * scale}
                height={pageSize.height * scale}
                scale={scale}
                root={scroller}
              />
            ))}
          </div>
        ) : (
          <div className="grid h-full place-items-center text-[12px] text-content/45">
            Rendering PDF…
          </div>
        )}
      </div>
      <footer className="flex h-8 shrink-0 items-center gap-3 border-t border-stroke px-3 text-[11px] text-content/50">
        <span className="tabular-nums">
          {doc ? `Page ${currentPage} of ${doc.numPages}` : "—"}
        </span>
        <span className="tabular-nums">{formatFileSize(size)}</span>
        <span className="uppercase">pdf</span>
        <span className="flex-1" />
        <ZoomButton label="Zoom out" onClick={() => stepZoom(1 / 1.25)}>
          <Minus className="size-3" strokeWidth={1.75} />
        </ZoomButton>
        <button
          type="button"
          title="Fit to width"
          onClick={() => setZoom("fit")}
          className="w-11 rounded text-center tabular-nums hover:text-content"
        >
          {zoom === "fit" ? "Fit" : `${Math.round(zoom * 100)}%`}
        </button>
        <ZoomButton label="Zoom in" onClick={() => stepZoom(1.25)}>
          <Plus className="size-3" strokeWidth={1.75} />
        </ZoomButton>
      </footer>
    </div>
  );
}

function PdfPage({
  doc,
  number,
  width,
  height,
  scale,
  root,
}: {
  doc: PDFDocumentProxy;
  number: number;
  width: number;
  height: number;
  scale: number;
  root: React.RefObject<HTMLDivElement | null>;
}) {
  const frame = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [near, setNear] = useState(false);

  useEffect(() => {
    const element = frame.current;
    if (!element) return;
    const observer = new IntersectionObserver(
      ([entry]) => setNear(entry.isIntersecting),
      { root: root.current, rootMargin: RENDER_MARGIN },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [root]);

  useEffect(() => {
    const target = canvas.current;
    if (!target) return;
    if (!near) {
      // Free the bitmap of a page far from view; a long PDF at 2x pixel
      // density would otherwise hold several megabytes per page.
      target.width = 0;
      target.height = 0;
      return;
    }
    let cancelled = false;
    let task: { cancel: () => void } | null = null;

    // A short delay lets a burst of zoom clicks settle before drawing.
    const timer = window.setTimeout(() => {
      void doc.getPage(number).then((page) => {
        if (cancelled) return;
        const ratio = window.devicePixelRatio || 1;
        const viewport = page.getViewport({ scale: scale * ratio });
        // Draw offscreen and copy over, so the old rendering stays visible
        // until the new one is complete.
        const offscreen = document.createElement("canvas");
        offscreen.width = Math.floor(viewport.width);
        offscreen.height = Math.floor(viewport.height);
        const render = page.render({ canvas: offscreen, viewport });
        task = render;
        render.promise.then(
          () => {
            if (cancelled) return;
            target.width = offscreen.width;
            target.height = offscreen.height;
            target.getContext("2d")?.drawImage(offscreen, 0, 0);
          },
          () => {
            // Cancelled renders reject; there is nothing to show for them.
          },
        );
      });
    }, 30);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      task?.cancel();
    };
  }, [doc, number, scale, near]);

  return (
    <div
      ref={frame}
      data-page={number}
      className="shrink-0 bg-white shadow-sm ring-1 ring-black/10"
      style={{ width, height }}
    >
      <canvas
        ref={canvas}
        aria-label={`Page ${number}`}
        className="block size-full"
      />
    </div>
  );
}

function describePdfError(cause: unknown): string {
  if (cause instanceof Error && cause.name === "PasswordException") {
    return "This PDF is password protected.";
  }
  return cause instanceof Error ? cause.message : String(cause);
}

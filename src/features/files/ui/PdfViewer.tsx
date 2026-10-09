import { useCallback, useEffect, useRef, useState } from "react";
import * as pdfjsLib from "pdfjs-dist/legacy/build/pdf.mjs";
import pdfWorkerUrl from "pdfjs-dist/legacy/build/pdf.worker.min.mjs?url";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  AlertCircle,
  ChevronLeft,
  ChevronRight,
  ExternalLink,
  Folder,
  Minus,
  Plus,
  RotateCcw,
} from "../../../shared/ui/icons";
import {
  formatFileSize,
  isSafeExternalUrl,
  sniffPdf,
} from "../model/filePreview";
import { watchFile } from "../model/fileWatch";
import {
  basename,
  openPathWithDefaultApp,
  readBinaryFile,
  revealPath,
} from "../../../platform/tauri/fs";
import { copyText } from "../../../platform/tauri/clipboard";
import { displayPath } from "../../../shared/lib/paths";
import { isRemoteProjectPath } from "../../projects/model/recents";

// Configure pdf.js worker with Vite's ?url bundled asset
pdfjsLib.GlobalWorkerOptions.workerSrc = pdfWorkerUrl;

const MIN_ZOOM = 0.25;
const MAX_ZOOM = 4.0;
const ZOOM_STEP = 1.25;

type Props = {
  path: string;
  cwd: string;
};

type LoadState =
  | { status: "loading" }
  | { status: "ready"; doc: pdfjsLib.PDFDocumentProxy; size: number }
  | { status: "unsupported"; size: number; reason: string }
  | { status: "error"; message: string };

export function PdfViewer({ path, cwd }: Props) {
  const [loadState, setLoadState] = useState<LoadState>({ status: "loading" });
  const [reloadKey, setReloadKey] = useState(0);

  const reload = useCallback(() => setReloadKey((v) => v + 1), []);

  useEffect(() => {
    let timer = 0;
    const stop = watchFile(path, () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(reload, 50);
    });
    return () => {
      window.clearTimeout(timer);
      stop();
    };
  }, [path, reload]);

  useEffect(() => {
    let cancelled = false;
    let loadingTask: pdfjsLib.PDFDocumentLoadingTask | null = null;
    setLoadState({ status: "loading" });

    void (async () => {
      try {
        const bytes = await readBinaryFile(path);
        if (cancelled) return;

        if (!sniffPdf(bytes)) {
          setLoadState({
            status: "unsupported",
            size: bytes.byteLength,
            reason: "File does not contain valid PDF header magic (%PDF-).",
          });
          return;
        }

        // Pass a copy/subslice of Uint8Array buffer to pdfjsLib
        const data = new Uint8Array(bytes);
        loadingTask = pdfjsLib.getDocument({
          data,
          cMapUrl: `${import.meta.env.BASE_URL}pdfjs/cmaps/`,
          cMapPacked: true,
          standardFontDataUrl: `${import.meta.env.BASE_URL}pdfjs/standard_fonts/`,
          wasmUrl: `${import.meta.env.BASE_URL}pdfjs/wasm/`,
        });

        const doc = await loadingTask.promise;
        if (cancelled) {
          void loadingTask.destroy();
          return;
        }
        setLoadState({ status: "ready", doc, size: bytes.byteLength });
      } catch (err: unknown) {
        if (cancelled) return;
        setLoadState({
          status: "error",
          message: err instanceof Error ? err.message : String(err),
        });
      }
    })();

    return () => {
      cancelled = true;
      if (loadingTask) {
        void loadingTask.destroy();
      }
    };
  }, [path, reloadKey]);

  if (loadState.status === "loading") {
    return (
      <div className="grid h-full place-items-center text-[12px] text-content/45">
        Opening {basename(path)}…
      </div>
    );
  }

  if (loadState.status === "error") {
    return (
      <PdfCard
        path={path}
        cwd={cwd}
        title={`Couldn’t open ${basename(path)}`}
        detail={loadState.message}
        icon={<AlertCircle className="mx-auto mb-3 size-5 text-red-400" />}
        onRetry={reload}
      />
    );
  }

  if (loadState.status === "unsupported") {
    return (
      <PdfCard
        path={path}
        cwd={cwd}
        title={basename(path)}
        detail={`${formatFileSize(loadState.size)} · ${loadState.reason}`}
        icon={<AlertCircle className="mx-auto mb-3 size-5 text-amber-400" />}
        onRetry={reload}
      />
    );
  }

  return (
    <PdfCanvasDocument
      doc={loadState.doc}
      path={path}
      cwd={cwd}
      size={loadState.size}
    />
  );
}

function PdfCanvasDocument({
  doc,
  path,
  cwd,
  size,
}: {
  doc: pdfjsLib.PDFDocumentProxy;
  path: string;
  cwd: string;
  size: number;
}) {
  const [currentPage, setCurrentPage] = useState(1);
  const [scale, setScale] = useState(1.0);
  const [renderError, setRenderError] = useState<string | null>(null);
  const [openError, setOpenError] = useState<string | null>(null);

  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const textLayerRef = useRef<HTMLDivElement | null>(null);
  const renderTaskRef = useRef<pdfjsLib.RenderTask | null>(null);
  const renderGeneration = useRef(0);

  const numPages = doc.numPages;

  const renderPage = useCallback(
    async (pageNum: number, pageScale: number) => {
      if (!canvasRef.current) return;
      const generation = ++renderGeneration.current;
      try {
        setRenderError(null);
        if (renderTaskRef.current) {
          renderTaskRef.current.cancel();
        }

        const page = await doc.getPage(pageNum);
        if (generation !== renderGeneration.current) return;
        await renderTaskRef.current?.promise.catch(() => {});
        if (generation !== renderGeneration.current) return;
        const viewport = page.getViewport({ scale: pageScale });

        const canvas = canvasRef.current;
        if (!canvas) return;
        const context = canvas.getContext("2d");
        if (!context) return;

        const outputScale = window.devicePixelRatio || 1;
        canvas.width = Math.floor(viewport.width * outputScale);
        canvas.height = Math.floor(viewport.height * outputScale);
        canvas.style.width = `${Math.floor(viewport.width)}px`;
        canvas.style.height = `${Math.floor(viewport.height)}px`;

        const transform =
          outputScale !== 1
            ? [outputScale, 0, 0, outputScale, 0, 0]
            : undefined;

        const renderContext = {
          canvas,
          canvasContext: context,
          transform: transform as number[] | undefined,
          viewport,
        };

        const renderTask = page.render(renderContext);
        renderTaskRef.current = renderTask;
        await renderTask.promise;
        if (generation !== renderGeneration.current) return;

        // Render annotations / link layer if available
        if (textLayerRef.current) {
          const container = textLayerRef.current;
          const annotations = await page.getAnnotations();
          if (generation !== renderGeneration.current) return;
          container.replaceChildren();
          container.style.width = `${Math.floor(viewport.width)}px`;
          container.style.height = `${Math.floor(viewport.height)}px`;
          for (const item of annotations) {
            if (item.subtype === "Link" && item.url) {
              const url = String(item.url);
              if (isSafeExternalUrl(url)) {
                const matrix = new DOMMatrix(viewport.transform);
                const start = matrix.transformPoint({ x: item.rect[0], y: item.rect[1] });
                const end = matrix.transformPoint({ x: item.rect[2], y: item.rect[3] });
                const rect = [start.x, start.y, end.x, end.y];
                const minX = Math.min(rect[0], rect[2]);
                const minY = Math.min(rect[1], rect[3]);
                const width = Math.abs(rect[2] - rect[0]);
                const height = Math.abs(rect[3] - rect[1]);

                const link = document.createElement("a");
                link.href = url;
                link.target = "_blank";
                link.rel = "noopener noreferrer";
                link.style.position = "absolute";
                link.style.left = `${minX}px`;
                link.style.top = `${minY}px`;
                link.style.width = `${width}px`;
                link.style.height = `${height}px`;
                link.style.cursor = "pointer";
                link.title = url;
                link.onclick = (e) => {
                  e.preventDefault();
                  setOpenError(null);
                  void openUrl(url).catch((cause: unknown) => {
                    setOpenError(cause instanceof Error ? cause.message : String(cause));
                  });
                };
                container.appendChild(link);
              }
            }
          }
        }
      } catch (err: unknown) {
        if (generation !== renderGeneration.current) return;
        if (
          err &&
          typeof err === "object" &&
          "name" in err &&
          err.name === "RenderingCancelledException"
        ) {
          return;
        }
        setRenderError(err instanceof Error ? err.message : String(err));
      }
    },
    [doc],
  );

  useEffect(() => {
    void renderPage(currentPage, scale);
    return () => {
      renderGeneration.current += 1;
      renderTaskRef.current?.cancel();
    };
  }, [currentPage, scale, renderPage]);

  const zoomIn = () => setScale((s) => Math.min(MAX_ZOOM, s * ZOOM_STEP));
  const zoomOut = () => setScale((s) => Math.max(MIN_ZOOM, s / ZOOM_STEP));
  const zoomReset = () => setScale(1.0);

  const prevPage = () => setCurrentPage((p) => Math.max(1, p - 1));
  const nextPage = () => setCurrentPage((p) => Math.min(numPages, p + 1));

  const isRemote = isRemoteProjectPath(path) || isRemoteProjectPath(cwd);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="grid min-h-0 flex-1 place-items-center overflow-auto overscroll-contain bg-content/5 p-6">
        <div className="relative shadow-md">
          <canvas ref={canvasRef} className="block rounded bg-white" />
          <div ref={textLayerRef} className="pointer-events-auto absolute inset-0 overflow-hidden" />
        </div>
        {renderError || openError ? (
          <div className="max-w-md text-center">
            <AlertCircle className="mx-auto mb-2 size-5 text-red-400" />
            <p role="alert" className="text-[12px] text-content/60">{renderError || openError}</p>
          </div>
        ) : null}
      </div>

      <footer className="flex h-8 shrink-0 items-center gap-2 border-t border-stroke px-3 text-[11px] text-content/50">
        <span className="tabular-nums">{formatFileSize(size)}</span>
        <span className="text-content/25">·</span>
        <div className="flex items-center gap-1">
          <ControlButton
            label="Previous page"
            onClick={prevPage}
            disabled={currentPage <= 1}
          >
            <ChevronLeft className="size-3" strokeWidth={1.75} />
          </ControlButton>
          <span className="tabular-nums">
            {currentPage} / {numPages}
          </span>
          <ControlButton
            label="Next page"
            onClick={nextPage}
            disabled={currentPage >= numPages}
          >
            <ChevronRight className="size-3" strokeWidth={1.75} />
          </ControlButton>
        </div>

        <span className="flex-1" />

        <div className="flex items-center gap-1">
          <ControlButton label="Zoom out" onClick={zoomOut}>
            <Minus className="size-3" strokeWidth={1.75} />
          </ControlButton>
          <button
            type="button"
            title="Reset zoom"
            onClick={zoomReset}
            className="w-12 rounded text-center tabular-nums hover:text-content"
          >
            {Math.round(scale * 100)}%
          </button>
          <ControlButton label="Zoom in" onClick={zoomIn}>
            <Plus className="size-3" strokeWidth={1.75} />
          </ControlButton>
        </div>

        {!isRemote ? (
          <ControlButton
            label="Open in external viewer"
            onClick={() => void openPathWithDefaultApp(path).catch(() => {})}
          >
            <ExternalLink className="size-3" strokeWidth={1.75} />
          </ControlButton>
        ) : null}
      </footer>
    </div>
  );
}

function ControlButton({
  label,
  onClick,
  disabled = false,
  children,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      className="grid size-5 place-items-center rounded hover:bg-content/10 hover:text-content disabled:opacity-30 disabled:hover:bg-transparent"
    >
      {children}
    </button>
  );
}

function PdfCard({
  path,
  cwd,
  title,
  detail,
  icon,
  onRetry,
}: {
  path: string;
  cwd: string;
  title: string;
  detail: string;
  icon: React.ReactNode;
  onRetry?: () => void;
}) {
  const isRemote = isRemoteProjectPath(path) || isRemoteProjectPath(cwd);
  return (
    <div className="grid h-full place-items-center p-6">
      <div className="max-w-md text-center">
        {icon}
        <p className="text-[13px] text-content">{title}</p>
        <p className="mt-1 text-[12px] leading-5 text-content/50">{detail}</p>
        <p className="mt-1 truncate font-mono text-[11px] text-content/35">
          {displayPath(path, cwd)}
        </p>
        <div className="mt-4 flex items-center justify-center gap-2">
          {onRetry ? (
            <CardButton onClick={onRetry}>
              <RotateCcw className="size-3" strokeWidth={1.75} />
              Retry
            </CardButton>
          ) : null}
          {!isRemote ? (
            <CardButton
              onClick={() => void openPathWithDefaultApp(path).catch(() => {})}
            >
              <ExternalLink className="size-3" strokeWidth={1.75} />
              Open externally
            </CardButton>
          ) : null}
          <CardButton onClick={() => void revealPath(path).catch(() => {})}>
            <Folder className="size-3" strokeWidth={1.75} />
            Reveal
          </CardButton>
          <CardButton onClick={() => void copyText(path).catch(() => {})}>
            Copy path
          </CardButton>
        </div>
      </div>
    </div>
  );
}

function CardButton({
  onClick,
  children,
}: {
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex h-7 items-center gap-1.5 rounded-md bg-content/10 px-2.5 text-[12px] text-content hover:bg-content/15"
    >
      {children}
    </button>
  );
}

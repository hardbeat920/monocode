import { useCallback, useEffect, useRef, useState } from "react";
import {
  AlertCircle,
  Check,
  Copy,
  Folder,
  Minus,
  Plus,
  RotateCcw,
} from "../../../shared/ui/icons";
import { ExplorerMenu } from "./ExplorerMenu";
import { FileTypeIcon } from "./FileTypeIcon";
import { PdfView } from "./PdfView";
import { clampZoom, ZoomButton } from "./ViewerControls";
import { copyText } from "../../../platform/tauri/clipboard";
import {
  formatFileSize,
  isPdfBytes,
  sniffImageMime,
} from "../model/filePreview";
import { watchFile } from "../model/fileWatch";
import {
  basename,
  copyFileToClipboard,
  readBinaryFile,
  revealPath,
} from "../../../platform/tauri/fs";
import { displayPath } from "../../../shared/lib/paths";
import { IS_MAC } from "../../../platform/tauri/platform";

type Props = { path: string; cwd: string };

type LoadState =
  | { status: "loading" }
  | { status: "ready"; url: string; mime: string; size: number }
  | { status: "pdf"; bytes: Uint8Array; size: number }
  | { status: "unsupported"; size: number }
  | { status: "error"; message: string };

/**
 * Read-only surface for files the editor can't open. Images and PDFs render;
 * bytes that turn out to be neither get a card pointing at the file on disk.
 */
export function BinaryFileView({ path, cwd }: Props) {
  const [state, setState] = useState<LoadState>({ status: "loading" });
  const [reloadKey, setReloadKey] = useState(0);

  useEffect(() => {
    let cancelled = false;
    let created: string | null = null;
    setState({ status: "loading" });

    readBinaryFile(path).then(
      (bytes) => {
        if (cancelled) return;
        if (isPdfBytes(bytes)) {
          setState({ status: "pdf", bytes, size: bytes.byteLength });
          return;
        }
        // The blob's MIME comes from the bytes, never the extension, so a file
        // named `.png` that holds markup can't become a same-origin document.
        const mime = sniffImageMime(bytes);
        if (!mime) {
          setState({ status: "unsupported", size: bytes.byteLength });
          return;
        }
        created = URL.createObjectURL(new Blob([bytes], { type: mime }));
        setState({
          status: "ready",
          url: created,
          mime,
          size: bytes.byteLength,
        });
      },
      (cause: unknown) => {
        if (cancelled) return;
        setState({
          status: "error",
          message: cause instanceof Error ? cause.message : String(cause),
        });
      },
    );

    return () => {
      cancelled = true;
      if (created) URL.revokeObjectURL(created);
    };
  }, [path, reloadKey]);

  const reload = useCallback(() => setReloadKey((value) => value + 1), []);

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

  if (state.status === "loading") {
    return (
      <div className="grid h-full place-items-center text-[12px] text-content/45">
        Opening {basename(path)}…
      </div>
    );
  }

  if (state.status === "error") {
    return (
      <FileCard
        path={path}
        cwd={cwd}
        title={`Couldn’t open ${basename(path)}`}
        detail={state.message}
        icon={<AlertCircle className="mx-auto mb-3 size-5 text-red-400" />}
        onRetry={reload}
      />
    );
  }

  if (state.status === "unsupported") {
    return (
      <FileCard
        path={path}
        cwd={cwd}
        title={basename(path)}
        detail={`${formatFileSize(state.size)} · not a readable image or PDF`}
        icon={
          <div className="mx-auto mb-3 flex justify-center">
            <FileTypeIcon name={basename(path)} isDir={false} size={28} />
          </div>
        }
      />
    );
  }

  if (state.status === "pdf") {
    return (
      <PdfView
        bytes={state.bytes}
        size={state.size}
        onError={(message) => setState({ status: "error", message })}
      />
    );
  }

  return (
    <ImageView
      path={path}
      url={state.url}
      size={state.size}
      mime={state.mime}
    />
  );
}

function ImageView({
  path,
  url,
  size,
  mime,
}: {
  path: string;
  url: string;
  size: number;
  mime: string;
}) {
  const [natural, setNatural] = useState<{ w: number; h: number } | null>(null);
  const [zoom, setZoom] = useState<number | "fit">("fit");
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [copied, setCopied] = useState(false);
  const copiedTimer = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (copiedTimer.current != null) window.clearTimeout(copiedTimer.current);
    },
    [],
  );

  const copyOriginal = useCallback(() => {
    setMenu(null);
    void copyFileToClipboard(path).then(
      () => {
        setCopied(true);
        if (copiedTimer.current != null) {
          window.clearTimeout(copiedTimer.current);
        }
        copiedTimer.current = window.setTimeout(() => setCopied(false), 1500);
      },
      (error: unknown) => {
        console.error("Failed to copy image file:", error);
      },
    );
  }, [path]);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div
        className="grid min-h-0 flex-1 place-items-center overflow-auto overscroll-contain p-4"
        style={{
          // A checkerboard so transparent PNGs read as transparent rather than
          // as whatever the theme background happens to be.
          backgroundImage:
            "linear-gradient(45deg, rgba(128,128,128,0.10) 25%, transparent 25%, transparent 75%, rgba(128,128,128,0.10) 75%), linear-gradient(45deg, rgba(128,128,128,0.10) 25%, transparent 25%, transparent 75%, rgba(128,128,128,0.10) 75%)",
          backgroundSize: "16px 16px",
          backgroundPosition: "0 0, 8px 8px",
        }}
      >
        <img
          src={url}
          alt=""
          draggable={false}
          onLoad={(event) =>
            setNatural({
              w: event.currentTarget.naturalWidth,
              h: event.currentTarget.naturalHeight,
            })
          }
          onClick={() => setZoom((value) => (value === "fit" ? 1 : "fit"))}
          onContextMenu={
            IS_MAC
              ? (event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  setMenu({ x: event.clientX, y: event.clientY });
                }
              : undefined
          }
          className={
            zoom === "fit"
              ? "max-h-full max-w-full object-contain"
              : "max-w-none object-contain"
          }
          style={
            zoom === "fit" || !natural
              ? undefined
              : { width: natural.w * zoom, height: natural.h * zoom }
          }
        />
      </div>
      <footer className="flex h-8 shrink-0 items-center gap-3 border-t border-stroke px-3 text-[11px] text-content/50">
        <span className="tabular-nums">
          {natural ? `${natural.w} × ${natural.h}` : "—"}
        </span>
        <span className="tabular-nums">{formatFileSize(size)}</span>
        <span className="uppercase">{mime.replace(/^image\//, "")}</span>
        <span className="flex-1" />
        {IS_MAC ? (
          <ZoomButton
            label={copied ? "Copied" : "Copy original file"}
            onClick={copyOriginal}
          >
            {copied ? (
              <Check className="size-3" strokeWidth={2} />
            ) : (
              <Copy className="size-3" strokeWidth={1.75} />
            )}
          </ZoomButton>
        ) : null}
        <ZoomButton
          label="Zoom out"
          onClick={() =>
            setZoom((value) => clampZoom((value === "fit" ? 1 : value) / 1.5))
          }
        >
          <Minus className="size-3" strokeWidth={1.75} />
        </ZoomButton>
        <button
          type="button"
          title="Fit to window"
          onClick={() => setZoom("fit")}
          className="w-11 rounded text-center tabular-nums hover:text-content"
        >
          {zoom === "fit" ? "Fit" : `${Math.round(zoom * 100)}%`}
        </button>
        <ZoomButton
          label="Zoom in"
          onClick={() =>
            setZoom((value) => clampZoom((value === "fit" ? 1 : value) * 1.5))
          }
        >
          <Plus className="size-3" strokeWidth={1.75} />
        </ZoomButton>
      </footer>
      {menu ? (
        <ExplorerMenu
          x={menu.x}
          y={menu.y}
          items={[
            {
              kind: "item",
              id: "copy-original",
              label: "Copy Original File",
            },
          ]}
          ariaLabel="Image actions"
          onPick={(id) => {
            if (id === "copy-original") copyOriginal();
          }}
          onClose={() => setMenu(null)}
        />
      ) : null}
    </div>
  );
}

function FileCard({
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

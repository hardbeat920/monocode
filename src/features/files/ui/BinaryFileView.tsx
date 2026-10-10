import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
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
import { copyText } from "../../../platform/tauri/clipboard";
import {
  formatFileSize,
  sniffImageMime,
  videoMimeForPath,
} from "../model/filePreview";
import { watchFile } from "../model/fileWatch";
import {
  basename,
  copyFileToClipboard,
  openPathWithDefaultApp,
  readBinaryFile,
  REMOTE_PATH_PREFIX,
  revealPath,
} from "../../../platform/tauri/fs";
import { displayPath } from "../../../shared/lib/paths";
import { IS_MAC } from "../../../platform/tauri/platform";
import { claimTrackpadMagnify } from "../../../platform/tauri/trackpadZoom";
import { resolveZoomKeybinding } from "../../settings/model/zoomKeybinding";

const MIN_ZOOM = 0.1;
const MAX_ZOOM = 16;
const ZOOM_STEP = 1.5;

type Zoom = number | "fit";
/** An image point that must stay under a client point after a zoom renders. */
type ZoomAnchor = {
  imageX: number;
  imageY: number;
  clientX: number;
  clientY: number;
};

type Props = { path: string; cwd: string; visible?: boolean };

type LoadState =
  | { status: "loading" }
  | { status: "ready"; url: string; mime: string; size: number }
  | { status: "unsupported"; size: number }
  | { status: "error"; message: string };

/**
 * Read-only image and video previews, with a card for unreadable media.
 */
export function BinaryFileView({ path, cwd, visible = true }: Props) {
  const [state, setState] = useState<LoadState>({ status: "loading" });
  const [reloadKey, setReloadKey] = useState(0);

  useEffect(() => {
    let cancelled = false;
    let created: string | null = null;
    setState({ status: "loading" });

    readBinaryFile(path).then(
      (bytes) => {
        if (cancelled) return;
        // Images are sniffed; videos receive only an explicit video MIME and
        // are validated by the media decoder. Neither can become a document.
        const mime = videoMimeForPath(path) ?? sniffImageMime(bytes);
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
        offerOpen={videoMimeForPath(path) !== null}
      />
    );
  }

  if (state.status === "unsupported") {
    return (
      <FileCard
        path={path}
        cwd={cwd}
        title={basename(path)}
        detail={`${formatFileSize(state.size)} · not a readable image`}
        icon={
          <div className="mx-auto mb-3 flex justify-center">
            <FileTypeIcon name={basename(path)} isDir={false} size={28} />
          </div>
        }
      />
    );
  }

  if (state.mime.startsWith("video/")) {
    return (
      <VideoView
        key={state.url}
        path={path}
        cwd={cwd}
        url={state.url}
        size={state.size}
        visible={visible}
        onRetry={reload}
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

function VideoView({
  path,
  cwd,
  url,
  size,
  visible,
  onRetry,
}: {
  path: string;
  cwd: string;
  url: string;
  size: number;
  visible: boolean;
  onRetry: () => void;
}) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [metadata, setMetadata] = useState<{
    width: number;
    height: number;
    duration: number;
  } | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const video = videoRef.current;
    const pauseWhenHidden = () => {
      if (!visible || document.hidden) video?.pause();
    };
    pauseWhenHidden();
    document.addEventListener("visibilitychange", pauseWhenHidden);
    return () =>
      document.removeEventListener("visibilitychange", pauseWhenHidden);
  }, [visible, failed]);

  useEffect(() => {
    const video = videoRef.current;
    // Restore the source and restart loading if React replays effects after cleanup.
    if (video && !video.hasAttribute("src")) {
      video.src = url;
      video.load();
    }
    return () => {
      if (!video) return;
      video.pause();
      video.removeAttribute("src");
      video.load();
    };
  }, [url, failed]);

  if (failed) {
    return (
      <FileCard
        path={path}
        cwd={cwd}
        title={`Couldn’t play ${basename(path)}`}
        detail="This video’s format or codec is unsupported, or the file is damaged."
        icon={<AlertCircle className="mx-auto mb-3 size-5 text-red-400" />}
        onRetry={onRetry}
        offerOpen
      />
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex min-h-0 flex-1 items-center justify-center bg-black/80 p-4">
        <video
          ref={videoRef}
          src={url}
          controls
          playsInline
          preload="metadata"
          aria-label={`Video preview: ${basename(path)}`}
          className="h-full w-full object-contain"
          onLoadedMetadata={(event) => {
            const video = event.currentTarget;
            setMetadata({
              width: video.videoWidth,
              height: video.videoHeight,
              duration: video.duration,
            });
          }}
          onPlay={(event) => {
            if (!visible || document.hidden) event.currentTarget.pause();
          }}
          onError={(event) => {
            if (event.currentTarget.hasAttribute("src")) setFailed(true);
          }}
        />
      </div>
      <footer className="flex h-8 shrink-0 items-center gap-3 border-t border-stroke px-3 text-[11px] text-content/50">
        <span className="tabular-nums">
          {metadata ? `${metadata.width} × ${metadata.height}` : "—"}
        </span>
        {metadata && Number.isFinite(metadata.duration) ? (
          <span className="tabular-nums">
            {Math.floor(metadata.duration / 60)}:
            {String(Math.floor(metadata.duration % 60)).padStart(2, "0")}
          </span>
        ) : null}
        <span className="tabular-nums">{formatFileSize(size)}</span>
        <span className="uppercase">{basename(path).split(".").pop()}</span>
      </footer>
    </div>
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
  const [zoom, setZoomState] = useState<Zoom>("fit");
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [copied, setCopied] = useState(false);
  const [hovered, setHovered] = useState(false);
  const copiedTimer = useRef<number | null>(null);
  const scrollerRef = useRef<HTMLDivElement>(null);
  const imageRef = useRef<HTMLImageElement>(null);
  const zoomRef = useRef<Zoom>(zoom);
  const anchorRef = useRef<ZoomAnchor | null>(null);
  const pointerRef = useRef<{ x: number; y: number } | null>(null);

  const setZoom = useCallback((next: Zoom) => {
    anchorRef.current = null;
    zoomRef.current = next;
    setZoomState(next);
  }, []);

  /** The rendered scale, including what "fit" works out to right now. */
  const currentScale = useCallback(() => {
    const value = zoomRef.current;
    if (value !== "fit") return value;
    const image = imageRef.current;
    if (!image?.naturalWidth) return 1;
    return image.getBoundingClientRect().width / image.naturalWidth;
  }, []);

  // Zooms so the image point under (clientX, clientY) stays there.
  const zoomAt = useCallback(
    (next: number, clientX: number, clientY: number) => {
      const image = imageRef.current;
      if (!image) return;
      const scale = currentScale();
      const rect = image.getBoundingClientRect();
      setZoom(clampZoom(next));
      anchorRef.current = {
        imageX: (clientX - rect.left) / scale,
        imageY: (clientY - rect.top) / scale,
        clientX,
        clientY,
      };
    },
    [currentScale, setZoom],
  );

  const zoomAtCenter = useCallback(
    (next: number) => {
      const scroller = scrollerRef.current;
      if (!scroller) return;
      const rect = scroller.getBoundingClientRect();
      zoomAt(next, rect.left + rect.width / 2, rect.top + rect.height / 2);
    },
    [zoomAt],
  );

  // A pinch carries no position, so it zooms toward the last pointer spot.
  const zoomAtPointer = useCallback(
    (next: number) => {
      const pointer = pointerRef.current;
      if (pointer) zoomAt(next, pointer.x, pointer.y);
      else zoomAtCenter(next);
    },
    [zoomAt, zoomAtCenter],
  );

  useLayoutEffect(() => {
    const anchor = anchorRef.current;
    const image = imageRef.current;
    const scroller = scrollerRef.current;
    anchorRef.current = null;
    if (!anchor || !image || !scroller || zoom === "fit") return;
    const rect = image.getBoundingClientRect();
    scroller.scrollLeft += rect.left + anchor.imageX * zoom - anchor.clientX;
    scroller.scrollTop += rect.top + anchor.imageY * zoom - anchor.clientY;
  }, [zoom]);

  // Cmd/Ctrl+scroll zooms; a plain scroll still pans the scroller.
  useEffect(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    const onWheel = (event: WheelEvent) => {
      if (!event.ctrlKey && !event.metaKey) return;
      event.preventDefault();
      const unit = event.deltaMode === WheelEvent.DOM_DELTA_LINE ? 16 : 1;
      const factor = Math.exp(-event.deltaY * unit * 0.01);
      zoomAt(currentScale() * factor, event.clientX, event.clientY);
    };
    scroller.addEventListener("wheel", onWheel, { passive: false });
    return () => scroller.removeEventListener("wheel", onWheel);
  }, [currentScale, zoomAt]);

  // While the pointer is over the preview, pinches and the zoom keys act on
  // the image instead of the app.
  useEffect(() => {
    if (!hovered) return;
    const release = claimTrackpadMagnify((delta) =>
      zoomAtPointer(currentScale() * (1 + delta)),
    );
    const onKeyDown = (event: KeyboardEvent) => {
      const action = resolveZoomKeybinding(event);
      if (!action) return;
      event.preventDefault();
      event.stopPropagation();
      if (action === "zoom-reset") setZoom("fit");
      else
        zoomAtCenter(
          action === "zoom-in"
            ? currentScale() * ZOOM_STEP
            : currentScale() / ZOOM_STEP,
        );
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      release();
      window.removeEventListener("keydown", onKeyDown, true);
    };
  }, [hovered, currentScale, setZoom, zoomAtCenter, zoomAtPointer]);

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
    <div
      className="flex h-full min-h-0 flex-col"
      data-image-zoom={hovered ? "" : undefined}
      onPointerEnter={() => setHovered(true)}
      onPointerLeave={() => {
        setHovered(false);
        pointerRef.current = null;
      }}
      onPointerMove={(event) => {
        pointerRef.current = { x: event.clientX, y: event.clientY };
      }}
    >
      <div
        ref={scrollerRef}
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
          ref={imageRef}
          src={url}
          alt=""
          draggable={false}
          onLoad={(event) =>
            setNatural({
              w: event.currentTarget.naturalWidth,
              h: event.currentTarget.naturalHeight,
            })
          }
          onClick={(event) => {
            if (zoomRef.current === "fit")
              zoomAt(1, event.clientX, event.clientY);
            else setZoom("fit");
          }}
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
          onClick={() => zoomAtCenter(currentScale() / ZOOM_STEP)}
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
          onClick={() => zoomAtCenter(currentScale() * ZOOM_STEP)}
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

function ZoomButton({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className="grid size-5 place-items-center rounded hover:bg-content/10 hover:text-content"
    >
      {children}
    </button>
  );
}

function FileCard({
  path,
  cwd,
  title,
  detail,
  icon,
  onRetry,
  offerOpen = false,
}: {
  path: string;
  cwd: string;
  title: string;
  detail: string;
  icon: React.ReactNode;
  onRetry?: () => void;
  offerOpen?: boolean;
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
          {offerOpen && !path.startsWith(REMOTE_PATH_PREFIX) ? (
            <CardButton
              onClick={() => void openPathWithDefaultApp(path).catch(() => {})}
            >
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

function clampZoom(value: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, value));
}

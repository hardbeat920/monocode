import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  fetchInboxMedia,
  sniffInboxMedia,
  type InboxMediaType,
} from "../../inbox/model/inboxMedia";
import { basename, readBinaryFile } from "../../../platform/tauri/fs";
import { ImageLightbox } from "../../../shared/ui/ImageLightbox";

type Props = {
  /** An allowlisted remote URL, or a local path when `local` is set. */
  src: string;
  alt?: string;
  /** The `width` an author gave an HTML `<img>`, as GitHub screenshots carry. */
  width?: number | string;
  local?: boolean;
};

type LoadState =
  | { status: "loading" }
  | { status: "ready"; url: string; type: InboxMediaType }
  | { status: "error" };

export function MarkdownMedia({ src, alt, width, local }: Props) {
  const [state, setState] = useState<LoadState>({ status: "loading" });
  const [zoomed, setZoomed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let objectUrl: string | null = null;
    setState({ status: "loading" });

    void (local ? readBinaryFile(src) : fetchInboxMedia(src))
      .then((bytes) => {
        const type = sniffInboxMedia(bytes);
        if (!type) throw new Error("unsupported");
        const url = URL.createObjectURL(new Blob([bytes], { type: type.mime }));
        if (cancelled) {
          URL.revokeObjectURL(url);
          return;
        }
        objectUrl = url;
        setState({ status: "ready", url, type });
      })
      .catch(() => {
        if (!cancelled) setState({ status: "error" });
      });

    return () => {
      cancelled = true;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [src, local]);

  if (state.status === "loading") {
    return (
      <span
        className="markdown-media my-2 inline-block h-32 w-72 max-w-full animate-pulse rounded-[10px] border border-content/10 bg-content/6 align-middle"
        role="status"
        aria-label="Loading image"
      />
    );
  }

  if (state.status === "error") {
    return local ? (
      <span className="text-content/45">
        {alt?.trim() || basename(src)} (image unavailable)
      </span>
    ) : (
      <RemoteMediaLink src={src} alt={alt} />
    );
  }

  if (state.type.kind === "video") {
    return (
      <span className="markdown-media my-2 inline-block w-full max-w-xl overflow-hidden rounded-[10px] border border-content/10 bg-content/6 align-middle">
        <video
          src={state.url}
          controls
          playsInline
          preload="metadata"
          className="block max-h-[28rem] w-full bg-black"
        >
          {local ? null : <RemoteMediaLink src={src} alt={alt} />}
        </video>
      </span>
    );
  }

  const label = alt?.trim() || (local ? basename(src) : "Image");
  return (
    <>
      <button
        type="button"
        aria-label={`Enlarge ${label}`}
        title={label}
        className="markdown-media my-2 inline-block max-w-full cursor-zoom-in overflow-hidden rounded-[10px] border border-content/10 bg-content/6 align-middle focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        // The author's width is a cap, so two screenshots side by side in a
        // table shrink to fit instead of scrolling the table sideways.
        style={
          cssWidth(width)
            ? { width: "100%", maxWidth: cssWidth(width) }
            : undefined
        }
        onClick={(event) => {
          // Media can sit inside a link; the button opens the preview instead.
          event.preventDefault();
          event.stopPropagation();
          setZoomed(true);
        }}
      >
        <img
          src={state.url}
          alt={alt ?? ""}
          draggable={false}
          className={`block h-auto max-h-[28rem] max-w-full object-contain ${
            cssWidth(width) ? "w-full" : "w-auto"
          }`}
        />
      </button>
      {zoomed ? (
        <ImageLightbox
          src={state.url}
          alt={label}
          caption={alt?.trim() || (local ? basename(src) : undefined)}
          onClose={() => setZoomed(false)}
        />
      ) : null}
    </>
  );
}

function cssWidth(width: number | string | undefined): string | undefined {
  if (typeof width === "string" && /^\d+(\.\d+)?%$/.test(width.trim())) {
    return width.trim();
  }
  const value =
    typeof width === "number" ? width : Number.parseFloat(width ?? "");
  return Number.isFinite(value) && value > 0 ? `${value}px` : undefined;
}

function RemoteMediaLink({ src, alt }: { src: string; alt?: string }) {
  return (
    <a
      href={src}
      className="text-sky-400/90 hover:text-sky-300 hover:underline"
      onClick={(event) => {
        event.preventDefault();
        void openUrl(src);
      }}
    >
      {alt?.trim() || src}
    </a>
  );
}

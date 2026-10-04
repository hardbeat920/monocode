import { useEffect, useState } from "react";
import { readBinaryFile } from "../../../platform/tauri/fs";
import { formatFileSize, sniffImageMime } from "../../files/model/filePreview";
import { ImageLightbox } from "../../../shared/ui/ImageLightbox";
import type { GeneratedImageMeta } from "../model/session";

/** The name Claude's computer-use bridge gives its screenshots. */
const COMPUTER_USE_SCREENSHOT = "Claude computer-use screenshot";

type State =
  | { status: "loading" }
  | { status: "ready"; url: string; size: number }
  | { status: "error" };

export function GeneratedImage({
  image,
  inline = false,
}: {
  image: GeneratedImageMeta;
  /** Shown inside a tool row: aligned with its text, not the transcript. */
  inline?: boolean;
}) {
  const [state, setState] = useState<State>({ status: "loading" });
  const [open, setOpen] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let created: string | null = null;
    setState({ status: "loading" });
    void readBinaryFile(image.path).then(
      (bytes) => {
        if (cancelled) return;
        const mime = sniffImageMime(bytes);
        if (!mime) {
          setState({ status: "error" });
          return;
        }
        created = URL.createObjectURL(new Blob([bytes], { type: mime }));
        setState({ status: "ready", url: created, size: bytes.byteLength });
      },
      () => {
        if (!cancelled) setState({ status: "error" });
      },
    );
    return () => {
      cancelled = true;
      if (created) URL.revokeObjectURL(created);
    };
  }, [image.path]);

  // Desktop screenshots are reference shots, not the answer. Keep them about
  // a third smaller than generated art; the lightbox still shows full size.
  const screenshot = image.name === COMPUTER_USE_SCREENSHOT;
  const noun = screenshot ? "screenshot" : "generated image";
  const frame = inline ? "min-w-0 pb-2 pl-5 pt-1" : "min-w-0 px-4 pb-3 pt-3";

  if (state.status === "loading") {
    return (
      <div className={`${frame} text-xs text-content/45`} role="status">
        Loading {noun}…
      </div>
    );
  }

  if (state.status === "error") {
    return (
      <div className={`${frame} text-xs text-content/50`} role="alert">
        Could not open {noun}.
      </div>
    );
  }

  const alt = image.alt || image.name;
  return (
    <div className={frame}>
      <button
        type="button"
        aria-label={`Open ${image.name} full screen`}
        title={`Open ${image.name} full screen`}
        onClick={() => setOpen(true)}
        className={`block cursor-zoom-in ${screenshot ? "max-w-[65%]" : "max-w-full"} overflow-hidden rounded-xl border border-content/10 bg-content/5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent`}
      >
        <img
          src={state.url}
          alt={alt}
          draggable={false}
          className={`${screenshot ? "max-h-[min(45vh,420px)]" : "max-h-[min(70vh,640px)]"} max-w-full object-contain`}
        />
      </button>
      <div className="mt-1 flex min-w-0 items-center gap-2 text-[11px] text-content/45">
        <span className="truncate">{image.name}</span>
        <span>{formatFileSize(state.size)}</span>
      </div>
      {open ? (
        <ImageLightbox
          src={state.url}
          alt={alt}
          onClose={() => setOpen(false)}
        />
      ) : null}
    </div>
  );
}

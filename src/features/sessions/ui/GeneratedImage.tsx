import { useEffect, useState } from "react";
import { readBinaryFile } from "../../../platform/tauri/fs";
import { formatFileSize, sniffImageMime } from "../../files/model/filePreview";
import { ImageLightbox } from "../../../shared/ui/ImageLightbox";
import type { GeneratedImageMeta } from "../model/session";

type State =
  | { status: "loading" }
  | { status: "ready"; url: string; size: number }
  | { status: "error" };

export function GeneratedImage({ image }: { image: GeneratedImageMeta }) {
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

  if (state.status === "loading") {
    return (
      <div className="px-4 py-3 text-xs text-content/45" role="status">
        Loading generated image…
      </div>
    );
  }

  if (state.status === "error") {
    return (
      <div className="px-4 py-3 text-xs text-content/50" role="alert">
        Could not open generated image.
      </div>
    );
  }

  const alt = image.alt || image.name;
  return (
    <div className="min-w-0 px-4 pb-3 pt-3">
      <button
        type="button"
        aria-label={`Open ${image.name} full screen`}
        title={`Open ${image.name} full screen`}
        onClick={() => setOpen(true)}
        className="block max-w-full cursor-zoom-in overflow-hidden rounded-xl border border-content/10 bg-content/5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
      >
        <img
          src={state.url}
          alt={alt}
          draggable={false}
          className="max-h-[min(70vh,640px)] max-w-full object-contain"
        />
      </button>
      <div className="mt-1 flex min-w-0 items-center gap-2 text-[11px] text-content/45">
        <span className="truncate">{image.name}</span>
        <span>{formatFileSize(state.size)}</span>
      </div>
      {open ? (
        <ImageLightbox src={state.url} alt={alt} onClose={() => setOpen(false)} />
      ) : null}
    </div>
  );
}

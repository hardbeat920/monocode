import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  fetchInboxMedia,
  sniffInboxMedia,
  type InboxMediaType,
} from "../lib/inboxMedia";

type Props = {
  src: string;
  alt?: string;
};

type LoadState =
  | { status: "loading" }
  | { status: "ready"; url: string; type: InboxMediaType }
  | { status: "error" };

export function InboxMedia({ src, alt }: Props) {
  const [state, setState] = useState<LoadState>({ status: "loading" });

  useEffect(() => {
    let cancelled = false;
    let objectUrl: string | null = null;
    setState({ status: "loading" });

    void fetchInboxMedia(src)
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
  }, [src]);

  if (state.status === "loading") {
    return (
      <span
        className="inbox-media my-2 inline-block h-32 w-full max-w-xl animate-pulse rounded-[10px] border border-content/10 bg-content/6"
        aria-hidden
      />
    );
  }

  if (state.status === "error") {
    return <MediaFallback src={src} alt={alt} />;
  }

  if (state.type.kind === "video") {
    return (
      <span className="inbox-media my-2 inline-block w-full max-w-xl overflow-hidden rounded-[10px] border border-content/10 bg-content/6">
        <video
          src={state.url}
          controls
          playsInline
          preload="metadata"
          className="block max-h-[28rem] w-full bg-black"
        >
          <MediaFallback src={src} alt={alt} />
        </video>
      </span>
    );
  }

  const label = alt?.trim() || "Image";
  return (
    <img
      src={state.url}
      alt={alt ?? ""}
      title={label}
      draggable={false}
      className="inbox-media my-2 inline-block max-h-[28rem] w-full max-w-xl cursor-zoom-in rounded-[10px] border border-content/10 bg-content/6 object-contain"
      onClick={(event) => {
        event.preventDefault();
        event.stopPropagation();
        void openUrl(src);
      }}
    />
  );
}

function MediaFallback({ src, alt }: { src: string; alt?: string }) {
  const label = alt?.trim() || src;
  return (
    <a
      href={src}
      className="text-sky-400/90 hover:text-sky-300 hover:underline"
      onClick={(event) => {
        event.preventDefault();
        void openUrl(src);
      }}
    >
      {label}
    </a>
  );
}

import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  fetchLinkPreviewMetadata,
  type LinkPreviewMetadata,
  type UserLink,
} from "../lib/linkPreview";

export function UserLinkPreview({ link }: { link: UserLink }) {
  const [metadata, setMetadata] = useState<LinkPreviewMetadata | null>(null);
  const [faviconFailed, setFaviconFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setMetadata(null);
    setFaviconFailed(false);
    void fetchLinkPreviewMetadata(link.url)
      .then((value) => {
        if (!cancelled) setMetadata(value);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [link.url]);

  const title = metadata?.title?.trim() || link.host;
  const favicon = faviconFailed ? null : metadata?.faviconDataUrl;

  return (
    <a
      href={link.url}
      data-user-link-preview
      title={link.url}
      aria-label={`Open ${title}`}
      className="user-link-preview group mx-0.5 text-sky-400/90 hover:text-sky-300 hover:underline"
      onClick={(event) => {
        event.preventDefault();
        event.stopPropagation();
        void openUrl(link.url).catch((error) => {
          console.error("Failed to open web link:", error);
        });
      }}
    >
      <span className="mr-1 inline-flex size-4 items-center justify-center overflow-hidden rounded bg-background-base/50 align-[-0.125em] text-[9px] font-semibold uppercase text-content/55">
        {favicon ? (
          <img
            src={favicon}
            alt=""
            draggable={false}
            className="size-3.5 object-contain"
            onError={() => setFaviconFailed(true)}
          />
        ) : (
          link.host.charAt(0)
        )}
      </span>
      <span className="user-link-preview-title font-medium group-hover:underline">
        {title}
      </span>
    </a>
  );
}

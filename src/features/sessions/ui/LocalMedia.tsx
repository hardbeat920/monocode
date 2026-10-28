import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { readBinaryFile, REMOTE_PATH_PREFIX } from "../../../platform/tauri/fs";
import {
  isImagePath,
  sniffImageMime,
  videoMimeForPath,
} from "../../files/model/filePreview";

/** Whether a path names a file `LocalMedia` can show. */
export function isLocalMediaPath(path: string): boolean {
  return isImagePath(path) || videoMimeForPath(path) !== null;
}

type Media = { url: string; kind: "image" | "video" };

/**
 * A local image or video, shown in place of the reply's link to it. Files load
 * once they scroll near view, so a long history doesn't read every file up
 * front. Local videos stream through the asset protocol, which allows just that
 * file, so their size doesn't matter; images, and files on a connected
 * machine, are read whole within the preview limit. Images are sniffed and
 * videos take only an explicit video MIME, as in the file viewer, so neither
 * can become a document. Anything unreadable falls back to the link it
 * replaced.
 */
export function LocalMedia({
  path,
  label,
  onOpen,
  fallback,
}: {
  path: string;
  label: string;
  onOpen?: () => void;
  fallback: ReactNode;
}) {
  const boxRef = useRef<HTMLSpanElement>(null);
  const [near, setNear] = useState(false);
  const [media, setMedia] = useState<Media | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const box = boxRef.current;
    if (!box || near) return;
    if (typeof IntersectionObserver === "undefined") {
      setNear(true);
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) setNear(true);
      },
      { rootMargin: "400px" },
    );
    observer.observe(box);
    return () => observer.disconnect();
  }, [near]);

  useEffect(() => {
    if (!near) return;
    let cancelled = false;
    let created: string | null = null;
    setMedia(null);
    setFailed(false);
    const fail = () => {
      if (!cancelled) setFailed(true);
    };
    if (videoMimeForPath(path) && !path.startsWith(REMOTE_PATH_PREFIX)) {
      invoke<string>("allow_video_stream", { path }).then((allowed) => {
        if (!cancelled)
          setMedia({ url: convertFileSrc(allowed), kind: "video" });
      }, fail);
      return () => {
        cancelled = true;
      };
    }
    readBinaryFile(path).then((bytes) => {
      if (cancelled) return;
      const video = videoMimeForPath(path);
      const mime = video ?? sniffImageMime(bytes);
      if (!mime) {
        setFailed(true);
        return;
      }
      created = URL.createObjectURL(new Blob([bytes], { type: mime }));
      setMedia({ url: created, kind: video ? "video" : "image" });
    }, fail);
    return () => {
      cancelled = true;
      if (created) URL.revokeObjectURL(created);
    };
  }, [near, path]);

  if (failed) return <>{fallback}</>;
  // One fixed 16:9 box from placeholder to playback, so loading never moves
  // the chat; media fits inside it, letterboxed.
  return (
    <span
      ref={boxRef}
      data-local-media={path}
      className="my-2 block aspect-video w-80 max-w-full overflow-hidden rounded-xl border border-content/10 bg-content/5"
    >
      {!media ? null : media.kind === "video" ? (
        <video
          src={media.url}
          controls
          playsInline
          preload="metadata"
          aria-label={label}
          className="block size-full bg-black object-contain"
          onError={() => setFailed(true)}
        />
      ) : (
        <img
          src={media.url}
          alt={label}
          title={onOpen ? "Open image" : undefined}
          draggable={false}
          onClick={onOpen}
          onError={() => setFailed(true)}
          className={`block size-full object-contain ${onOpen ? "cursor-zoom-in" : ""}`}
        />
      )}
    </span>
  );
}

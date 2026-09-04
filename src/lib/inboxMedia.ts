import { invoke } from "@tauri-apps/api/core";
import { sniffImageMime } from "./filePreview";

/**
 * Prefixes rehype-harden will keep on issue/PR markdown. The fetcher still
 * re-checks the host; this list only has to match the URLs GitHub and Linear
 * put in the markdown source, not the CDN they redirect to.
 */
export const INBOX_MEDIA_PREFIXES = [
  "https://github.com/user-attachments/",
  "https://www.github.com/user-attachments/",
  "https://user-images.githubusercontent.com/",
  "https://private-user-images.githubusercontent.com/",
  "https://objects.githubusercontent.com/",
  "https://media.githubusercontent.com/",
  "https://camo.githubusercontent.com/",
  "https://avatars.githubusercontent.com/",
  "https://raw.githubusercontent.com/",
  "https://gist.githubusercontent.com/",
  "https://uploads.linear.app/",
];

export type InboxMediaKind = "image" | "video";
export type InboxMediaType = { kind: InboxMediaKind; mime: string };

const mediaCache = new Map<string, Promise<Uint8Array>>();

/** Remote image/video URLs GitHub and Linear actually put in issue bodies. */
export function isInboxMediaUrl(value: string): boolean {
  let url: URL;
  try {
    url = new URL(value.trim());
  } catch {
    return false;
  }
  if (url.protocol !== "https:") return false;
  if (url.username || url.password) return false;
  const host = url.hostname.replace(/\.$/, "").toLowerCase();
  if (pathHasDotDot(url.pathname)) return false;
  if (host === "uploads.linear.app" || host.endsWith(".uploads.linear.app")) {
    return true;
  }
  if (
    host === "githubusercontent.com" ||
    host.endsWith(".githubusercontent.com")
  ) {
    return true;
  }
  if (host !== "github.com" && host !== "www.github.com") return false;
  const path = url.pathname.toLowerCase();
  if (path.startsWith("/user-attachments/")) return true;
  const parts = path.split("/").filter(Boolean);
  return (
    parts.length >= 4 &&
    parts[2] === "assets" &&
    /^[0-9]+$/.test(parts[3] ?? "")
  );
}

export function sniffInboxMedia(bytes: Uint8Array): InboxMediaType | null {
  const mime = sniffImageMime(bytes);
  if (mime) return { kind: "image", mime };
  return sniffVideoType(bytes);
}

export function fetchInboxMedia(url: string): Promise<Uint8Array> {
  const key = url.trim();
  const cached = mediaCache.get(key);
  if (cached) return cached;
  const pending = invoke<ArrayBuffer>("fetch_inbox_media", { url: key }).then(
    (buffer) => new Uint8Array(buffer),
  );
  mediaCache.set(key, pending);
  pending.catch(() => {
    mediaCache.delete(key);
  });
  return pending;
}

function sniffVideoType(bytes: Uint8Array): InboxMediaType | null {
  if (
    bytes.length >= 12 &&
    startsWith(bytes.subarray(4), [0x66, 0x74, 0x79, 0x70])
  ) {
    const brand = String.fromCharCode(...bytes.subarray(8, 12));
    if (brand === "avif" || brand === "avis") return null;
    return {
      kind: "video",
      mime: brand === "qt  " ? "video/quicktime" : "video/mp4",
    };
  }
  if (startsWith(bytes, [0x1a, 0x45, 0xdf, 0xa3])) {
    return { kind: "video", mime: "video/webm" };
  }
  return null;
}

function pathHasDotDot(path: string): boolean {
  return path.split("/").some((segment) => {
    const lower = segment.toLowerCase();
    return (
      lower === ".." ||
      lower === "%2e%2e" ||
      lower === "%2e." ||
      lower === ".%2e"
    );
  });
}

function startsWith(bytes: Uint8Array, magic: number[]): boolean {
  if (bytes.length < magic.length) return false;
  return magic.every((byte, index) => bytes[index] === byte);
}

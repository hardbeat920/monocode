import { invoke } from "@tauri-apps/api/core";
import { sniffImageMime } from "../../files/model/filePreview";

export type InboxMediaKind = "image" | "video";
export type InboxMediaType = { kind: InboxMediaKind; mime: string };

// Each file can be up to 25 MB, so keep recent bytes under a total budget
// instead of every image and video ever shown. Requests in flight are shared.
const MEDIA_CACHE_BYTES = 32 * 1024 * 1024;
const mediaCache = new Map<string, Uint8Array>();
const mediaRequests = new Map<string, Promise<Uint8Array>>();
let mediaCacheBytes = 0;

/**
 * Remote image/video URLs GitHub and Linear actually put in issue bodies.
 * Mirrors `is_allowed_media_target` in inbox_media.rs, which has the final say.
 */
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
  switch (parts[2]) {
    case "assets":
      return parts.length >= 4 && /^[0-9]+$/.test(parts[3] ?? "");
    case "raw":
    case "blob":
      return parts.length >= 5;
    case "releases":
      return parts.length >= 6 && parts[3] === "download";
    default:
      return false;
  }
}

export function sniffInboxMedia(bytes: Uint8Array): InboxMediaType | null {
  const mime = sniffImageMime(bytes);
  if (mime) return { kind: "image", mime };
  return sniffVideoType(bytes);
}

export function fetchInboxMedia(url: string): Promise<Uint8Array> {
  const key = url.trim();
  const cached = mediaCache.get(key);
  if (cached) {
    mediaCache.delete(key);
    mediaCache.set(key, cached);
    return Promise.resolve(cached);
  }
  const inFlight = mediaRequests.get(key);
  if (inFlight) return inFlight;
  const pending = invoke<ArrayBuffer>("fetch_inbox_media", { url: key }).then(
    (buffer) => {
      const bytes = new Uint8Array(buffer);
      rememberMedia(key, bytes);
      return bytes;
    },
  );
  mediaRequests.set(key, pending);
  const settle = () => {
    mediaRequests.delete(key);
  };
  pending.then(settle, settle);
  return pending;
}

function rememberMedia(key: string, bytes: Uint8Array) {
  if (bytes.byteLength > MEDIA_CACHE_BYTES) return;
  const previous = mediaCache.get(key);
  if (previous) {
    mediaCache.delete(key);
    mediaCacheBytes -= previous.byteLength;
  }
  mediaCache.set(key, bytes);
  mediaCacheBytes += bytes.byteLength;
  for (const [oldest, old] of mediaCache) {
    if (mediaCacheBytes <= MEDIA_CACHE_BYTES) break;
    mediaCache.delete(oldest);
    mediaCacheBytes -= old.byteLength;
  }
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

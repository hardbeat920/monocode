import { basename } from "../../../platform/tauri/fs";

const IMAGE_EXTENSIONS = new Set([
  ".png",
  ".jpg",
  ".jpeg",
  ".gif",
  ".webp",
  ".avif",
  ".bmp",
  ".ico",
]);

// "%PDF-"
const PDF_MAGIC = [0x25, 0x50, 0x44, 0x46, 0x2d];

/**
 * Whether a path belongs to the image viewer, decided before anything is read.
 *
 * `.svg` is deliberately absent: it is text, so it keeps opening in the editor,
 * which offers its own rendered preview alongside the source.
 */
export function isImagePath(path: string): boolean {
  return IMAGE_EXTENSIONS.has(extensionOf(path));
}

/** Whether a path belongs to the PDF viewer, decided before anything is read. */
export function isPdfPath(path: string): boolean {
  return extensionOf(path) === ".pdf";
}

/**
 * Whether bytes hold a PDF. The spec lets the `%PDF-` header sit anywhere in
 * the first 1024 bytes, and pdf.js accepts files that use that allowance.
 */
export function isPdfBytes(bytes: Uint8Array): boolean {
  const head = bytes.subarray(0, 1024);
  for (let index = 0; index + PDF_MAGIC.length <= head.length; index += 1) {
    if (startsWith(head.subarray(index), PDF_MAGIC)) return true;
  }
  return false;
}

/**
 * Identify image bytes by their magic number rather than trusting the name.
 *
 * The viewer builds a blob URL, and a blob's MIME decides how the webview
 * treats it. Sniffing means an HTML file named `logo.png` fails to match and
 * lands on the unsupported card instead of becoming a same-origin document.
 */
export function sniffImageMime(bytes: Uint8Array): string | null {
  if (startsWith(bytes, [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])) {
    return "image/png";
  }
  if (startsWith(bytes, [0xff, 0xd8, 0xff])) return "image/jpeg";
  if (startsWith(bytes, [0x47, 0x49, 0x46, 0x38])) return "image/gif";
  if (startsWith(bytes, [0x42, 0x4d])) return "image/bmp";
  if (startsWith(bytes, [0x00, 0x00, 0x01, 0x00])) return "image/x-icon";
  // RIFF....WEBP — the four size bytes at offset 4 are skipped.
  if (
    startsWith(bytes, [0x52, 0x49, 0x46, 0x46]) &&
    startsWith(bytes.subarray(8), [0x57, 0x45, 0x42, 0x50])
  ) {
    return "image/webp";
  }
  // ....ftyp{avif,avis} — an ISO base media box, shared with HEIF and MP4.
  if (startsWith(bytes.subarray(4), [0x66, 0x74, 0x79, 0x70])) {
    const brand = String.fromCharCode(...bytes.subarray(8, 12));
    if (brand === "avif" || brand === "avis") return "image/avif";
  }
  return null;
}

/** Byte count for the viewer footer, in the units a file manager would show. */
export function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let size = bytes / 1024;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size < 10 ? size.toFixed(1) : Math.round(size)} ${units[unit]}`;
}

function extensionOf(path: string): string {
  const name = basename(path).toLowerCase();
  return name.includes(".") ? name.slice(name.lastIndexOf(".")) : "";
}

function startsWith(bytes: Uint8Array, magic: number[]): boolean {
  if (bytes.length < magic.length) return false;
  return magic.every((byte, index) => bytes[index] === byte);
}

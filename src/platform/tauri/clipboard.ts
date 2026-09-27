import { invoke } from "@tauri-apps/api/core";
import {
  attachmentsFromFiles,
  attachmentsFromPaths,
  MAX_ATTACHMENTS,
  MAX_EMBED_BYTES,
} from "../../features/sessions/model/attachments";
import type { Attachment } from "../../features/sessions/model/session";

type CopiedFile = { name: string; mimeType: string; data: string };

const FILES_ATTRIBUTE = 'data-monocode-files="';
const MAX_CLIPBOARD_METADATA_CHARS = 64 * 1024;
const MAX_CLIPBOARD_BASE64_CHARS = Math.ceil(MAX_EMBED_BYTES / 3) * 4;
const MAX_CLIPBOARD_PAYLOAD_CHARS =
  MAX_CLIPBOARD_BASE64_CHARS * 3 + MAX_CLIPBOARD_METADATA_CHARS;
const MAX_CLIPBOARD_HTML_CHARS =
  MAX_CLIPBOARD_PAYLOAD_CHARS +
  MAX_CLIPBOARD_BASE64_CHARS +
  MAX_CLIPBOARD_METADATA_CHARS;

/** HTML keeps arbitrary files together with text across MonoCode windows. */
export async function copyMessage(
  text: string,
  attachments: Attachment[] = [],
): Promise<void> {
  const files: CopiedFile[] = [];
  for (const attachment of attachments) {
    if (
      attachment.kind === "image" &&
      !attachment.data &&
      !attachment.previewUrl &&
      !attachment.copyFromPath
    )
      continue;
    try {
      let data = attachment.data;
      if (data === undefined && attachment.path) {
        data = await invoke<string>("read_file_base64", {
          path: attachment.path,
        });
      }
      if (data === undefined)
        throw new Error("File content is no longer available.");
      files.push({
        name: attachment.name,
        mimeType: attachment.mimeType,
        data,
      });
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      throw new Error(`Could not copy ${attachment.name}: ${reason}`);
    }
  }
  if (!files.length) {
    if (!text) throw new Error("No copyable content is available.");
    return copyText(text);
  }
  const escape = (value: string) =>
    value.replace(
      /[&<>"']/g,
      (char) =>
        ({
          "&": "&amp;",
          "<": "&lt;",
          ">": "&gt;",
          '"': "&quot;",
          "'": "&#39;",
        })[char]!,
    );
  const html = `<div data-monocode-files="${encodeURIComponent(JSON.stringify(files))}"><pre>${escape(text)}</pre>${files
    .map((file) => {
      const src = `data:${escape(file.mimeType)};base64,${escape(file.data)}`;
      return file.mimeType.startsWith("image/")
        ? `<img src="${src}" alt="${escape(file.name)}">`
        : `<a href="${src}" download="${escape(file.name)}">${escape(file.name)}</a>`;
    })
    .join("")}</div>`;
  const formats: Record<string, Blob> = {
    "text/plain": new Blob([text], { type: "text/plain" }),
    "text/html": new Blob([html], { type: "text/html" }),
  };
  const png = files.find((file) => file.mimeType === "image/png");
  if (png)
    formats["image/png"] = new Blob(
      [Uint8Array.from(atob(png.data), (char) => char.charCodeAt(0))],
      { type: "image/png" },
    );
  await navigator.clipboard.write([new ClipboardItem(formats)]);
}

/** Only embedded bytes are accepted; clipboard HTML cannot request local paths. */
export function messageFilesFromClipboard(
  clipboard: Pick<DataTransfer, "getData">,
): File[] | null {
  try {
    const html = clipboard.getData("text/html");
    if (!html || html.length > MAX_CLIPBOARD_HTML_CHARS) return null;
    const payloadStart = html.indexOf(FILES_ATTRIBUTE);
    if (payloadStart < 0) return null;
    const valueStart = payloadStart + FILES_ATTRIBUTE.length;
    const valueEnd = html.indexOf('"', valueStart);
    if (valueEnd < 0) return null;
    const payload = html.slice(valueStart, valueEnd);
    if (!payload || payload.length > MAX_CLIPBOARD_PAYLOAD_CHARS) return null;
    const files: unknown = JSON.parse(decodeURIComponent(payload));
    if (!Array.isArray(files) || files.length > MAX_ATTACHMENTS) return null;

    let decodedBytes = 0;
    for (const file of files) {
      if (
        typeof file?.name !== "string" ||
        typeof file?.mimeType !== "string" ||
        typeof file?.data !== "string"
      )
        throw new Error("Invalid attachment");
      const padding = file.data.endsWith("==")
        ? 2
        : file.data.endsWith("=")
          ? 1
          : 0;
      decodedBytes += Math.floor((file.data.length * 3) / 4) - padding;
      if (decodedBytes > MAX_EMBED_BYTES) return null;
    }

    return files.map((file) => {
      const bytes = Uint8Array.from(atob(file.data), (char) =>
        char.charCodeAt(0),
      );
      return new File([bytes], file.name, { type: file.mimeType });
    });
  } catch {
    return null;
  }
}

export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const el = document.createElement("textarea");
    el.value = text;
    el.style.position = "fixed";
    el.style.left = "-9999px";
    document.body.appendChild(el);
    el.select();
    const ok = document.execCommand("copy");
    el.remove();
    if (!ok) throw new Error("copy failed");
  }
}

/**
 * Paths for files copied in a file manager, empty when it holds none.
 */
export async function readClipboardFilePaths(): Promise<string[]> {
  try {
    const paths = await invoke<string[]>("clipboard_file_paths");
    return Array.isArray(paths) ? paths.filter((path) => path.trim()) : [];
  } catch {
    return [];
  }
}

/**
 * Attachments for a paste the webview reported without a single file.
 *
 * `text` is what the webview saw on the clipboard. Copies made in a file
 * manager and screenshots reach us only through the native clipboard, so try
 * paths before image bytes, and leave a text paste alone.
 */
export async function nativeClipboardAttachments(
  text: string,
): Promise<Attachment[]> {
  const paths = await readClipboardFilePaths();
  if (paths.length) {
    const files = await attachmentsFromPaths(paths);
    if (files.length) return files;
    throw new Error(
      `Nothing to attach in ${
        paths.length === 1 ? "that path" : "those paths"
      }. Hidden system files are skipped.`,
    );
  }
  if (text) return [];
  return attachmentsFromFiles([await readClipboardImage()]);
}
/**
 * An image held by the native clipboard, as a `File`.
 *
 * A webview's paste event carries text only, so images copied by a screenshot
 * tool never reach `clipboardData.files`. Throws with a message worth showing
 * when the clipboard has no readable image; callers only ask when the paste
 * event carried neither a file nor text.
 */
export async function readClipboardImage(): Promise<File> {
  let buffer: ArrayBuffer;
  try {
    buffer = await invoke<ArrayBuffer>("clipboard_image");
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    throw new Error(reason || "The clipboard could not be read.");
  }
  if (!buffer?.byteLength)
    throw new Error("The clipboard does not contain an image.");
  return new File([buffer], "clipboard-image.png", { type: "image/png" });
}

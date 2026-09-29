import { invoke } from "@tauri-apps/api/core";
import {
  attachmentsFromFiles,
  attachmentsFromPaths,
  isAttachmentFolder,
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

/** Folder paths the plain text does not already list, one per line. */
function textWithFolderPaths(text: string, paths: string[]): string {
  const lines = new Set(text.split("\n"));
  const extra = paths.filter((path) => !lines.has(path));
  if (!extra.length) return text;
  const suffix = extra.join("\n");
  if (!text) return suffix;
  return text.endsWith("\n") ? text + suffix : `${text}\n${suffix}`;
}

/** HTML keeps arbitrary files together with text across MonoCode windows. */
export async function copyMessage(
  text: string,
  attachments: Attachment[] = [],
): Promise<void> {
  const files: CopiedFile[] = [];
  const folderPaths: string[] = [];
  for (const attachment of attachments) {
    // A folder has no bytes to copy, and asking for them fails the whole copy.
    // The path is the reference, so it goes in the copied text instead.
    if (isAttachmentFolder(attachment)) {
      const path = attachment.path?.trim();
      if (path) folderPaths.push(path);
      continue;
    }
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
  const payload = textWithFolderPaths(text, folderPaths);
  if (!files.length) {
    if (!payload) throw new Error("No copyable content is available.");
    return copyText(payload);
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
  const html = `<div data-monocode-files="${encodeURIComponent(JSON.stringify(files))}"><pre>${escape(payload)}</pre>${files
    .map((file) => {
      const src = `data:${escape(file.mimeType)};base64,${escape(file.data)}`;
      return file.mimeType.startsWith("image/")
        ? `<img src="${src}" alt="${escape(file.name)}">`
        : `<a href="${src}" download="${escape(file.name)}">${escape(file.name)}</a>`;
    })
    .join("")}</div>`;
  const formats: Record<string, Blob> = {
    "text/plain": new Blob([payload], { type: "text/plain" }),
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
 * True when the clipboard text is a file reference rather than prose, so a
 * file-manager copy is not mistaken for a text paste.
 *
 * A file manager that also publishes plain text publishes the path as a
 * `file://` URI, so that is the only shape treated as a reference. Anything
 * else — including a URL from "copy image" in a browser — is a text paste.
 */
export function isFileReferenceText(text: string): boolean {
  return text.trim().toLowerCase().startsWith("file:");
}

export type NativeClipboardPaste = {
  files: Attachment[];
  /** Set when the clipboard held more copies than one turn can carry. */
  warning?: string;
};

/**
 * Attachments for copied paths, filling a turn's quota with whatever the
 * filesystem actually accepts, plus how many paths it took to get there.
 *
 * A path that was moved or deleted yields no attachment, so paths are taken a
 * batch at a time until the quota is met. Counting them before converting would
 * drop a good file that sat behind an unreadable one.
 */
async function attachmentsFromClipboardPaths(paths: string[]) {
  const files: Attachment[] = [];
  let consumed = 0;
  while (consumed < paths.length && files.length < MAX_ATTACHMENTS) {
    const batch = paths.slice(consumed, consumed + MAX_ATTACHMENTS);
    for (const file of await attachmentsFromPaths(batch)) {
      if (files.length >= MAX_ATTACHMENTS) break;
      files.push(file);
    }
    consumed += batch.length;
  }
  return { files, consumed };
}

/**
 * Attachments for a paste the webview reported without a single file.
 *
 * `text` is what the webview saw on the clipboard. Copies made in a file
 * manager and screenshots reach us only through the native clipboard, so try
 * paths before image bytes; an image is only worth reading when the paste
 * carried no text at all.
 */
export async function nativeClipboardAttachments(
  text: string,
): Promise<NativeClipboardPaste> {
  const paths = await readClipboardFilePaths();
  if (paths.length) {
    const { files, consumed } = await attachmentsFromClipboardPaths(paths);
    if (!files.length)
      throw new Error(
        `Nothing to attach from ${
          paths.length === 1 ? "that path" : "those paths"
        } — the file may have been moved, renamed, or deleted.`,
      );
    return {
      files,
      ...(consumed < paths.length
        ? {
            warning: `Attached ${files.length} of ${paths.length} copied files. A turn carries up to ${MAX_ATTACHMENTS}.`,
          }
        : {}),
    };
  }
  if (text) return { files: [] };
  try {
    return { files: await attachmentsFromFiles([await readClipboardImage()]) };
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    // An empty clipboard is a no-op. A real read failure still surfaces.
    if (reason === "The clipboard does not contain an image.") return { files: [] };
    throw error;
  }
}

/**
 * Paths for files copied in a file manager, empty when the clipboard holds
 * none. Rejects when the clipboard cannot be read, so a failure reaches the
 * composer instead of looking like an empty clipboard.
 *
 * Invokes the command directly rather than through `fs.clipboardFilePaths`:
 * that module pulls in the dialog plugin, which nothing here needs.
 */
export async function readClipboardFilePaths(): Promise<string[]> {
  const paths = await invoke<string[]>("clipboard_file_paths");
  return Array.isArray(paths) ? paths.filter((path) => path.trim()) : [];
}

/**
 * An image held by the native clipboard, as a `File`.
 *
 * A webview's paste event carries text only, so images copied by a screenshot
 * tool never reach `clipboardData.files`. Throws with a message worth showing
 * when the clipboard has no readable image.
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

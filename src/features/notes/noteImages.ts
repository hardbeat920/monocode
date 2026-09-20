import { invoke } from "@tauri-apps/api/core";
import {
  attachmentsFromFiles,
  attachmentsFromPaths,
  revokeAttachment,
} from "../sessions/model/attachments";
import type { Attachment } from "../sessions/model/session";

export const NOTE_IMAGE_PREFIX = "/note-assets/";

export type NoteImageAsset = {
  name: string;
  markdownPath: string;
};

export type MarkdownInsertion = {
  value: string;
  cursor: number;
};

export async function saveNoteImagesFromFiles(
  noteId: string,
  files: File[],
): Promise<NoteImageAsset[]> {
  return saveNoteImageAttachments(noteId, await attachmentsFromFiles(files));
}

export async function saveNoteImagesFromPaths(
  noteId: string,
  paths: string[],
): Promise<NoteImageAsset[]> {
  return saveNoteImageAttachments(noteId, await attachmentsFromPaths(paths));
}

async function saveNoteImageAttachments(
  noteId: string,
  attachments: Attachment[],
): Promise<NoteImageAsset[]> {
  const images = attachments.filter((file) => file.kind === "image");
  if (images.length === 0) {
    throw new Error("Drop a PNG, JPG, GIF, WebP, or SVG image.");
  }

  const saved: NoteImageAsset[] = [];
  let failure: unknown;
  try {
    for (const image of images) {
      let sourcePath = image.path;
      let temporary = false;
      if (!sourcePath && image.data) {
        sourcePath = await invoke<string>("write_attachment", {
          name: image.name,
          data: image.data,
        });
        temporary = true;
      }
      if (!sourcePath) continue;
      try {
        saved.push(
          await invoke<NoteImageAsset>("notes_save_image", {
            noteId,
            sourcePath,
          }),
        );
      } catch (err: unknown) {
        failure ??= err;
      } finally {
        if (temporary) {
          await invoke("delete_path", { path: sourcePath }).catch(
            () => undefined,
          );
        }
      }
    }
  } finally {
    for (const image of images) revokeAttachment(image);
  }

  if (saved.length === 0) {
    if (failure instanceof Error) throw failure;
    if (failure) throw new Error(String(failure));
    throw new Error("None of the dropped images could be added to the note.");
  }
  return saved;
}

export function noteImageMarkdown(image: NoteImageAsset): string {
  const alt = image.name
    .replace(/[\r\n]+/g, " ")
    .replace(/\\/g, "\\\\")
    .replace(/([\[\]])/g, "\\$1");
  return `![${alt}](${image.markdownPath})`;
}

export function insertNoteImagesMarkdown(
  value: string,
  start: number,
  end: number,
  images: NoteImageAsset[],
): MarkdownInsertion {
  if (images.length === 0) {
    const cursor = Math.max(0, Math.min(start, value.length));
    return { value, cursor };
  }

  const from = Math.max(0, Math.min(start, value.length));
  const to = Math.max(from, Math.min(end, value.length));
  const before = value.slice(0, from);
  const after = value.slice(to);
  const block = images.map(noteImageMarkdown).join("\n\n");
  const leading = before
    ? before.endsWith("\n\n")
      ? ""
      : before.endsWith("\n")
        ? "\n"
        : "\n\n"
    : "";
  const trailing = after
    ? after.startsWith("\n\n")
      ? ""
      : after.startsWith("\n")
        ? "\n"
        : "\n\n"
    : "";
  const inserted = `${leading}${block}`;

  return {
    value: `${before}${inserted}${trailing}${after}`,
    cursor: before.length + inserted.length,
  };
}

export function isNoteImagePath(value: string): boolean {
  return value.startsWith(NOTE_IMAGE_PREFIX);
}

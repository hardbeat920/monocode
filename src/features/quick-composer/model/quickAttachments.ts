import { invoke } from "@tauri-apps/api/core";
import {
  MAX_ATTACHMENTS,
  persistableAttachment,
} from "../../sessions/model/attachments";
import type { Attachment } from "../../sessions/model/session";

/** Paths survive the handoff to another webview; blob URLs do not. */
export async function storeQuickAttachments(
  files: Attachment[],
): Promise<Attachment[]> {
  return Promise.all(
    files.map(async (file) => {
      if (file.path) return file;
      if (!file.data) throw new Error(`Could not attach ${file.name}.`);
      const path = await invoke<string>("write_attachment", {
        name: file.name,
        data: file.data,
      });
      return { ...file, path };
    }),
  );
}

export function quickLaunchAttachments(files: Attachment[]): Attachment[] {
  return files.map((file) => {
    if (!file.path) throw new Error(`Could not attach ${file.name}.`);
    return persistableAttachment(file);
  });
}

export function parseQuickAttachments(value: unknown): Attachment[] | null {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.length > MAX_ATTACHMENTS) return null;
  const files: Attachment[] = [];
  for (const item of value) {
    if (
      !item ||
      typeof item !== "object" ||
      typeof item.id !== "string" ||
      !item.id ||
      typeof item.name !== "string" ||
      !item.name ||
      typeof item.mimeType !== "string" ||
      !item.mimeType ||
      !["image", "audio", "file"].includes(item.kind) ||
      typeof item.size !== "number" ||
      !Number.isSafeInteger(item.size) ||
      item.size < 0 ||
      typeof item.path !== "string" ||
      !item.path.trim()
    )
      return null;
    files.push(persistableAttachment(item));
  }
  return files;
}

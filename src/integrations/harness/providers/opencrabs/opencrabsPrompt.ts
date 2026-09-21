import { invoke } from "@tauri-apps/api/core";
import { fileUri, type PromptContentBlock } from "../../../../features/sessions/model/attachments";
import type { Attachment } from "../../../../features/sessions/model/session";

const EXT_BY_MIME: Record<string, string> = {
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/jpg": "jpg",
  "image/gif": "gif",
  "image/webp": "webp",
  "audio/mpeg": "mp3",
  "audio/mp3": "mp3",
  "audio/wav": "wav",
  "audio/ogg": "ogg",
  "audio/m4a": "m4a",
  "audio/webm": "webm",
  "application/pdf": "pdf",
  "text/plain": "txt",
  "text/markdown": "md",
};

/**
 * Prompt blocks shaped for `opencrabs acp`.
 *
 * The server declares `promptCapabilities.image: false` and reads
 * `resource_link` blocks as on-disk path references, so every attachment
 * kind — image, audio, file — is rewritten as a link. The rule is uniform:
 * a disk path links as-is; a pasted blob is persisted through the shared
 * `write_attachment` command first; an attachment with neither fails the
 * send with a visible error. Nothing is silently dropped.
 */
export async function openCrabsPromptBlocks(
  text: string,
  attachments: Attachment[] = [],
): Promise<PromptContentBlock[]> {
  const blocks: PromptContentBlock[] = [];
  const trimmed = text.trim();
  if (trimmed) blocks.push({ type: "text", text: trimmed });
  for (const file of attachments) {
    blocks.push(await attachmentBlock(file));
  }
  return blocks;
}

async function attachmentBlock(file: Attachment): Promise<PromptContentBlock> {
  if (file.path?.trim()) {
    return {
      type: "resource_link",
      uri: fileUri(file.path),
      name: file.name,
      mimeType: file.mimeType,
      size: file.size,
    };
  }
  if (!file.data) {
    throw new Error(
      `Cannot attach ${JSON.stringify(file.name)}: no local file path or data is available. Attach the file again.`,
    );
  }
  const name = persistName(file);
  const path = await invoke<string>("write_attachment", {
    name,
    data: file.data,
  });
  return {
    type: "resource_link",
    uri: fileUri(path),
    name,
    mimeType: file.mimeType,
    size: file.size,
  };
}

/** Pasted blobs get a generated name: kind plus the best extension guess. */
function persistName(file: Attachment): string {
  const ext =
    EXT_BY_MIME[file.mimeType] ??
    (file.name.includes(".") ? file.name.split(".").pop() : undefined) ??
    "bin";
  return `pasted-${file.kind}.${ext}`;
}

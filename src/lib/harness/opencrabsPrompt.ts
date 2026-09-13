import { invoke } from "@tauri-apps/api/core";
import {
  fileUri,
  promptBlocks,
  type PromptContentBlock,
} from "../attachments";
import type { Attachment } from "../session";

const EXT_BY_MIME: Record<string, string> = {
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/jpg": "jpg",
  "image/gif": "gif",
  "image/webp": "webp",
};

/**
 * Prompt blocks shaped for `opencrabs acp`.
 *
 * The server declares `promptCapabilities.image: false` and reads
 * `resource_link` blocks as on-disk path references, so vision blocks are
 * rewritten as links instead of being sent inline. Images that already
 * carry a local `uri` (picked from disk) convert for free; pasted blobs
 * are persisted through the shared `write_attachment` command first.
 * Nothing is silently dropped — a blob that cannot be persisted fails the
 * send with a visible error, matching the `attachmentPath` contract.
 */
export async function openCrabsPromptBlocks(
  text: string,
  attachments: Attachment[] = [],
): Promise<PromptContentBlock[]> {
  const blocks = promptBlocks(text, attachments);
  return Promise.all(blocks.map(asServerBlock));
}

async function asServerBlock(
  block: PromptContentBlock,
): Promise<PromptContentBlock> {
  if (block.type !== "image") return block;
  if (block.uri) {
    return {
      type: "resource_link",
      uri: block.uri,
      name: nameFromUri(block.uri),
      mimeType: block.mimeType,
    };
  }
  if (!block.data) {
    throw new Error(
      "Cannot attach image: no local file path or image data is available. Attach the file again.",
    );
  }
  const ext = EXT_BY_MIME[block.mimeType] ?? "png";
  const name = `pasted-image.${ext}`;
  const path = await invoke<string>("write_attachment", {
    name,
    data: block.data,
  });
  return {
    type: "resource_link",
    uri: fileUri(path),
    name,
    mimeType: block.mimeType,
  };
}

function nameFromUri(uri: string): string {
  const segments = uri.split("/").filter(Boolean);
  const leaf = segments[segments.length - 1];
  if (!leaf) return "image";
  try {
    return decodeURIComponent(leaf);
  } catch {
    return leaf;
  }
}

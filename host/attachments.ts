import {
  mkdirSync,
  openSync,
  closeSync,
  readSync,
  writeSync,
  statSync,
} from "node:fs";
import { join } from "node:path";
import type { Attachment } from "../src/features/sessions/model/session";
import type { RemoteAttachment } from "../src/features/connections/model/protocol";
import type { HostStore } from "./store";

export const MAX_REMOTE_ATTACHMENT_BYTES = 20 * 1024 * 1024;
const MAX_CHUNK_BYTES = 512 * 1024;
const UUID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export function attachmentPath(store: HostStore, id: string): string {
  if (!UUID.test(id)) throw new Error("Invalid attachment ID");
  return join(store.attachmentDir, id);
}

/** An offset makes a repeated chunk safe when its HTTP response was lost. */
export function writeAttachmentChunk(
  store: HostStore,
  input: Record<string, unknown>,
): { offset: number } {
  const path = attachmentPath(store, String(input.id ?? ""));
  const offset = input.offset;
  const size = input.size;
  const encoded = input.data;
  if (
    !Number.isSafeInteger(offset) ||
    Number(offset) < 0 ||
    !Number.isSafeInteger(size) ||
    Number(size) < 0 ||
    Number(size) > MAX_REMOTE_ATTACHMENT_BYTES ||
    typeof encoded !== "string" ||
    encoded.length > Math.ceil(MAX_CHUNK_BYTES / 3) * 4 ||
    (encoded.length > 0 &&
      (!/^[A-Za-z0-9+/]*={0,2}$/.test(encoded) || encoded.length % 4))
  )
    throw new Error("Invalid attachment chunk");
  const bytes = Buffer.from(encoded, "base64");
  if (
    bytes.length > MAX_CHUNK_BYTES ||
    Number(offset) + bytes.length > Number(size)
  )
    throw new Error("Invalid attachment chunk size");
  mkdirSync(store.attachmentDir, { recursive: true, mode: 0o700 });
  const fd = openSync(path, Number(offset) === 0 ? "a+" : "r+", 0o600);
  try {
    const length = statSync(path).size;
    if (length === Number(offset)) {
      writeSync(fd, bytes, 0, bytes.length, Number(offset));
    } else if (length >= Number(offset) + bytes.length) {
      const existing = Buffer.alloc(bytes.length);
      readSync(fd, existing, 0, existing.length, Number(offset));
      if (!existing.equals(bytes))
        throw new Error("Attachment retry does not match uploaded bytes");
    } else {
      throw new Error("Attachment chunks are out of order");
    }
    return { offset: Number(offset) + bytes.length };
  } finally {
    closeSync(fd);
  }
}

export function parseRemoteAttachments(value: unknown): RemoteAttachment[] {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.length > 20)
    throw new Error("Invalid attachments");
  return value.map((entry) => {
    if (!entry || typeof entry !== "object" || Array.isArray(entry))
      throw new Error("Invalid attachment");
    const v = entry as Record<string, unknown>;
    if (
      !UUID.test(String(v.id ?? "")) ||
      typeof v.name !== "string" ||
      !v.name.trim() ||
      v.name.length > 255 ||
      typeof v.mimeType !== "string" ||
      !v.mimeType.trim() ||
      v.mimeType.length > 128 ||
      !["image", "audio", "file"].includes(String(v.kind)) ||
      !Number.isSafeInteger(v.size) ||
      Number(v.size) < 0 ||
      Number(v.size) > MAX_REMOTE_ATTACHMENT_BYTES
    )
      throw new Error("Invalid attachment");
    return {
      id: v.id as string,
      name: v.name,
      mimeType: v.mimeType,
      kind: v.kind as RemoteAttachment["kind"],
      size: v.size as number,
    };
  });
}

export function resolveAttachments(
  store: HostStore,
  refs: RemoteAttachment[],
): Attachment[] {
  return refs.map((ref) => {
    const path = attachmentPath(store, ref.id);
    if (statSync(path).size !== ref.size)
      throw new Error(`Attachment ${ref.name} is incomplete`);
    return { ...ref, path };
  });
}

/** Reads only an attachment already accepted into this session. Paths supplied
 * by the client are never used, and each response stays below the RPC cap. */
export function readAttachmentChunk(store: HostStore, input: Record<string, unknown>) {
  const session = store.session(String(input.sessionId ?? ""));
  const attachment = session.session.blocks.flatMap((block) => block.attachments ?? [])
    .find((file) => file.id === input.id);
  if (!attachment || attachment.kind !== "image") throw new Error("Image attachment not found");
  const offset = input.offset;
  if (!Number.isSafeInteger(offset) || Number(offset) < 0 || Number(offset) > attachment.size)
    throw new Error("Invalid attachment offset");
  const path = attachmentPath(store, attachment.id);
  if (statSync(path).size !== attachment.size) throw new Error("Attachment is incomplete");
  // Non-final chunks are divisible by three, so the client can join base64.
  const bytes = Buffer.alloc(Math.min(3 * Math.floor(MAX_CHUNK_BYTES / 3), attachment.size - Number(offset)));
  const fd = openSync(path, "r");
  try {
    const read = readSync(fd, bytes, 0, bytes.length, Number(offset));
    return { data: bytes.subarray(0, read).toString("base64"), offset: Number(offset) + read, size: attachment.size };
  } finally { closeSync(fd); }
}

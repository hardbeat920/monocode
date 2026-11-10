import { attachmentsFromFiles } from "../../sessions/model/attachments";
import type { Attachment } from "../../sessions/model/session";

/** As the phone's composer allows; bounds what one command can stage. */
const MAX_MOBILE_ATTACHMENTS = 10;

/** True when a phone command carries any files, before any are staged. */
export function hasMobileAttachments(value: unknown): boolean {
  return Array.isArray(value) && value.length > 0;
}

/**
 * The files a phone sent with a message, as `{ name, mimeType, data }` in
 * base64. Staged as pasted files are here: vision images travel inline and
 * anything else is written to disk for the agent to read.
 */
export async function mobileAttachments(value: unknown): Promise<Attachment[]> {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.length > MAX_MOBILE_ATTACHMENTS)
    throw new Error(`Attach up to ${MAX_MOBILE_ATTACHMENTS} files`);
  const files = value.map((item: unknown) => {
    const { name, mimeType, data } = (item ?? {}) as Record<string, unknown>;
    if (typeof name !== "string" || !name.trim() || name.length > 255)
      throw new Error("An attachment is missing its name");
    if (typeof data !== "string") throw new Error(`${name} has no contents`);
    const type = typeof mimeType === "string" ? mimeType : "";
    return new File([decodeBase64(data, name)], name, { type });
  });
  const staged = await attachmentsFromFiles(files);
  if (staged.length !== files.length) throw new Error("Desktop could not save an attachment");
  return staged;
}

function decodeBase64(data: string, name: string): Uint8Array<ArrayBuffer> {
  let binary: string;
  try {
    binary = atob(data);
  } catch {
    throw new Error(`${name} could not be read`);
  }
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

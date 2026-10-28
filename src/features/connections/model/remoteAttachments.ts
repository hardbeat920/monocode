import { invoke } from "@tauri-apps/api/core";
import type { Attachment } from "../../sessions/model/session";
import type { RemoteAttachment } from "./protocol";
import { remoteRequest } from "./connections";

const MAX_BYTES = 20 * 1024 * 1024;
// Keep each request well below the host's 4 MiB JSON limit.
const CHUNK_CHARS = 4 * Math.floor((512 * 1024) / 3);

export async function uploadRemoteAttachments(
  machineId: string,
  attachments: Attachment[],
): Promise<RemoteAttachment[]> {
  if (attachments.length > 20) throw new Error("Too many attachments");
  const uploaded: RemoteAttachment[] = [];
  for (const file of attachments) {
    if (file.size > MAX_BYTES)
      throw new Error(
        `${file.name} is too large to send to a remote machine (20 MB maximum)`,
      );
    const data =
      file.data ??
      (file.path
        ? await invoke<string>("read_file_base64", { path: file.path })
        : undefined);
    if (data === undefined)
      throw new Error(`Cannot read ${file.name} for remote upload`);
    let offset = 0;
    if (data.length === 0) {
      await remoteRequest(machineId, "attachments.upload", {
        id: file.id,
        offset: 0,
        size: file.size,
        data: "",
      });
    }
    for (let index = 0; index < data.length; index += CHUNK_CHARS) {
      const chunk = data.slice(index, index + CHUNK_CHARS);
      const reply = await remoteRequest<{ offset: number }>(
        machineId,
        "attachments.upload",
        {
          id: file.id,
          offset,
          size: file.size,
          data: chunk,
        },
      );
      offset = reply.offset;
    }
    if (offset !== file.size)
      throw new Error(`Could not finish uploading ${file.name}`);
    uploaded.push({
      id: file.id,
      name: file.name,
      mimeType: file.mimeType,
      kind: file.kind,
      size: file.size,
    });
  }
  return uploaded;
}

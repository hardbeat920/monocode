import { invoke } from "@tauri-apps/api/core";
import type { Attachment } from "./session";
export { historicalContextAttachments, snapshotPortableContextAssets } from "./portableContext";

export type ContextAssetSnapshot = {
  id: string;
  path?: string;
  sha256?: string;
  unavailableReason?: string;
};

export function snapshotContextAssets(sessionId: string, attachments: Attachment[]): Promise<ContextAssetSnapshot[]> {
  return invoke("session_context_assets", {
    sessionId,
    attachments: attachments.map(({ id, name, path, data }) => ({ id, name, path, data })),
  });
}

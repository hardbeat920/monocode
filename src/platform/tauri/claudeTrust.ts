import { invoke } from "@tauri-apps/api/core";

/**
 * Whether Claude Code would open on its folder trust dialog for `folder`.
 *
 * Read from `~/.claude.json`, where the CLI records the answer per folder.
 */
export function claudeFolderTrusted(folder: string): Promise<boolean> {
  return invoke<boolean>("claude_folder_trusted", { folder });
}

/** Record `folder` as trusted, as answering the dialog with "Yes" would. */
export function claudeTrustFolder(folder: string): Promise<void> {
  return invoke<void>("claude_trust_folder", { folder });
}

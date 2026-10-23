import type { ProjectFile } from "../../../platform/tauri/fs";
import {
  sharedMentionIndex,
  mentionLabel,
  rankMentionFiles,
  withMentionDirectories,
} from "../../files/model/fileMentions";

/** Labels must come from the complete checkout index, never the filtered hits. */
export function mobileMentionResults(
  files: ProjectFile[],
  query: string,
  recents: string[] = [],
) {
  const index = sharedMentionIndex(files, []);
  // Bare @ browses the project root; typing searches the whole checkout.
  const candidates = query.trim()
    ? files
    : withMentionDirectories(files).filter(
        (file) => !file.relative.includes("/"),
      );
  return rankMentionFiles(candidates, query, query.trim() ? recents : []).map(
    (file) => ({
      path: file.relative,
      name: file.name,
      isDir: Boolean(file.isDir),
      positions: file.positions,
      label: mentionLabel(file, index),
    }),
  );
}

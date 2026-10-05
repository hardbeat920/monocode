import type { ChatContextItem } from "../../sessions/model/chatContext";

export type EditorCodeSelection = {
  path: string;
  startLine: number;
  endLine: number;
};

/** Turn an editor range into a reference the agent can read on demand. */
export function editorSelectionContext({
  path,
  startLine,
  endLine,
}: EditorCodeSelection): ChatContextItem {
  return {
    kind: "code",
    path: path.replace(/\\/g, "/"),
    startLine,
    endLine,
  };
}

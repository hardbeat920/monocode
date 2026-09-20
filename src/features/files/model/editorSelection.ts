export type EditorCodeSelection = {
  path: string;
  startLine: number;
  endLine: number;
};

/** Turn an editor range into a compact reference the agent can read on demand. */
export function formatEditorSelectionReference({
  path,
  startLine,
  endLine,
}: EditorCodeSelection): string {
  const file = formatFileReference(path);
  const lines =
    startLine === endLine
      ? `line ${startLine}`
      : `lines ${startLine}-${endLine}`;
  return `${file} (${lines})`;
}

function formatFileReference(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  if (isMentionablePath(normalized)) return `@${normalized}`;
  return `\`${normalized.replace(/[`\r\n]/g, "'")}\``;
}

function isMentionablePath(path: string): boolean {
  if (!path || path.length > 120 || path.startsWith("/")) return false;
  if (/\s|@|[\u0000-\u001f\u007f-\u009f]/.test(path)) return false;
  return path.split("/").every((part) => part && part !== "." && part !== "..");
}

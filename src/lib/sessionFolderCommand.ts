import type { BuiltinSkill } from "./skills";

export const SESSION_FOLDER_COMMAND: BuiltinSkill = {
  kind: "builtin",
  name: "add-to-folder",
  invocation: "add-to-folder",
  description: "Place this session in an existing or new sidebar folder.",
  scope: "builtin",
  source: "monocode",
};

/** Match the standalone composer command without consuming ordinary prompt text. */
export function isSessionFolderCommand(text: string): boolean {
  return /^\s*\/add-to-folder\s*$/i.test(text);
}

/** Remove the leading local command while preserving the user's actual prompt. */
export function consumeSessionFolderCommand(text: string): {
  text: string;
  matched: boolean;
} {
  const match = text.match(/^\s*\/add-to-folder(?=\s|$)\s*/i);
  if (!match) return { text, matched: false };
  return { text: text.slice(match[0].length), matched: true };
}

export function runsSessionFolderCommandOnSpace(input: {
  text: string;
  selectionStart: number;
  selectionEnd: number;
  altKey?: boolean;
  ctrlKey?: boolean;
  metaKey?: boolean;
}): boolean {
  return (
    !input.altKey &&
    !input.ctrlKey &&
    !input.metaKey &&
    input.selectionStart === input.text.length &&
    input.selectionEnd === input.text.length &&
    /^\s*\/add-to-folder$/i.test(input.text)
  );
}

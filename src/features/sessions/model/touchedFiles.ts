import {
  displayPath,
  pathKey,
  resolveWorkspaceFileReference,
  resolveWorkspacePath,
  slash,
  windowsPath,
} from "../../../shared/lib/paths";
import { REMOTE_PATH_PREFIX } from "../../../shared/lib/remotePaths";
import type { Block } from "./session";
import { isFailedStatus } from "./transcriptActivity";

// Roots a reference carries itself: `/x`, `\\host`, `C:/x`, `~/x`, `file://`.
const ABSOLUTE_REFERENCE = /^(?:[/\\~]|[A-Za-z]:[/\\]|file:|remote:)/i;

/** Absolute files the session's tool calls read, wrote or listed. */
export function sessionTouchedPaths(blocks: readonly Block[]): string[] {
  const paths = new Map<string, string>();
  for (const block of blocks) {
    const tool = block.role === "tool" ? block.tool : undefined;
    if (!tool || isFailedStatus(tool.status)) continue;
    for (const path of [...(tool.paths ?? []), tool.preview?.path]) {
      // A shell tool may record just `NOTES.md`, which names no file by itself.
      const absolute = path ? resolveWorkspacePath(path) : undefined;
      if (absolute) paths.set(pathKey(absolute), absolute);
    }
  }
  return [...paths.values()];
}

/**
 * Like `resolveWorkspaceFileReference`, but a reference that is relative to the
 * session's cwd resolves to the file the session's tools touched when exactly
 * one of them fits. An agent often names a file by its base name alone, after
 * working on it somewhere other than the cwd. Anything else resolves as usual.
 */
export function resolveSessionFileReference(
  reference: string,
  cwd: string | undefined,
  touchedPaths: readonly string[] = [],
): ReturnType<typeof resolveWorkspaceFileReference> {
  const file = resolveWorkspaceFileReference(reference, cwd);
  // An absolute reference already names its file, and touched paths are local
  // ones, so a connected machine's cwd can't use them.
  if (
    !file ||
    touchedPaths.length === 0 ||
    ABSOLUTE_REFERENCE.test(reference.trim()) ||
    cwd?.startsWith(REMOTE_PATH_PREFIX)
  ) {
    return file;
  }
  // Touched paths are absolute, so one that ends with the reference is that file.
  const tail = `/${displayPath(file.path, cwd)}`;
  // Windows paths compare without case, as `pathKey` does.
  const matches = touchedPaths.filter((path) =>
    windowsPath(path)
      ? pathKey(path).endsWith(tail.toLowerCase())
      : slash(path).endsWith(tail),
  );
  return matches.length === 1 ? { ...file, path: matches[0] } : file;
}

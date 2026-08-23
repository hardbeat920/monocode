import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

export type FsEntry = {
  name: string;
  path: string;
  isDir: boolean;
  ignored: boolean;
};

export type ProjectFile = {
  name: string;
  path: string;
  relative: string;
};

export function listDir(path: string): Promise<FsEntry[]> {
  return invoke<FsEntry[]>("list_dir", { path });
}

export type DiscoveredSkill = {
  name: string;
  description: string;
  path: string;
  scope: "project" | "user" | "builtin";
  source: "agents" | "claude" | "cursor" | "codex" | "opencode" | "pi" | "monocode";
};

export function listSkills(cwd: string): Promise<DiscoveredSkill[]> {
  return invoke<DiscoveredSkill[]>("list_skills", { cwd });
}

export function listProjectFiles(cwd: string): Promise<ProjectFile[]> {
  return invoke<ProjectFile[]>("list_project_files", { cwd });
}

export type GitDiffStats = {
  files: number;
  additions: number;
  deletions: number;
};

export function gitDiffStats(cwd: string): Promise<GitDiffStats> {
  return invoke<GitDiffStats>("git_diff_stats", { cwd });
}

export type GitChangedFile = {
  path: string;
  relative: string;
  status: "modified" | "added" | "deleted" | "untracked" | string;
  additions: number;
  deletions: number;
  staged: boolean;
  unstaged: boolean;
};

export type GitDiffIndex = {
  branch: string | null;
  files: GitChangedFile[];
  additions: number;
  deletions: number;
  remote: string | null;
  upstream: string | null;
  defaultBranch: string | null;
  ahead: number;
  behind: number;
  aheadOfDefault: number;
};

export function gitDiffIndex(cwd: string): Promise<GitDiffIndex> {
  return invoke<GitDiffIndex>("git_diff_index", { cwd });
}

export type GitFileDiff = {
  path: string;
  relative: string;
  status: string;
  original: string;
  current: string;
  binary: boolean;
  tooLarge: boolean;
};

export function gitFileDiff(cwd: string, relative: string): Promise<GitFileDiff> {
  return invoke<GitFileDiff>("git_file_diff", { cwd, relative });
}

export function gitStageContents(
  cwd: string,
  relative: string,
  contents: string,
): Promise<void> {
  return invoke<void>("git_stage_contents", { cwd, relative, contents });
}

export function gitStageFile(cwd: string, relative: string): Promise<void> {
  return invoke<void>("git_stage_file", { cwd, relative });
}

export function gitUnstageFile(cwd: string, relative: string): Promise<void> {
  return invoke<void>("git_unstage_file", { cwd, relative });
}

export function gitDiscardFile(cwd: string, relative: string): Promise<void> {
  return invoke<void>("git_discard_file", { cwd, relative });
}

export function gitStageAll(cwd: string): Promise<void> {
  return invoke<void>("git_stage_all", { cwd });
}

export function gitUnstageAll(cwd: string): Promise<void> {
  return invoke<void>("git_unstage_all", { cwd });
}

export function gitCommit(cwd: string, message: string): Promise<void> {
  return invoke<void>("git_commit", { cwd, message });
}

export type GitStagedContext = {
  branch: string | null;
  summary: string;
  patch: string;
};

export function gitStagedContext(cwd: string): Promise<GitStagedContext> {
  return invoke<GitStagedContext>("git_staged_context", { cwd });
}

export function gitPush(cwd: string): Promise<void> {
  return invoke<void>("git_push", { cwd });
}

export function gitPull(cwd: string): Promise<void> {
  return invoke<void>("git_pull", { cwd });
}

export function gitSync(cwd: string): Promise<void> {
  return invoke<void>("git_sync", { cwd });
}

export type GitRangeContext = {
  base: string;
  head: string;
  commitSummary: string;
  diffSummary: string;
  diffPatch: string;
};

export function gitRangeContext(cwd: string): Promise<GitRangeContext> {
  return invoke<GitRangeContext>("git_range_context", { cwd });
}

export type GitPr = {
  number: number;
  title: string;
  url: string;
  state: string;
};

export function gitPrStatus(cwd: string): Promise<GitPr | null> {
  return invoke<GitPr | null>("git_pr_status", { cwd });
}

export function gitPrCreate(
  cwd: string,
  title: string,
  body: string,
  base: string,
  head: string,
): Promise<string> {
  return invoke<string>("git_pr_create", { cwd, title, body, base, head });
}

export type GitBranchInfo = {
  name: string;
  current: boolean;
  remote: string | null;
};

export type GitBranches = {
  current: string | null;
  detached: boolean;
  branches: GitBranchInfo[];
};

export function gitBranches(cwd: string): Promise<GitBranches> {
  return invoke<GitBranches>("git_branches", { cwd });
}

export function gitCheckout(
  cwd: string,
  name: string,
  remote?: string | null,
): Promise<string> {
  return invoke<string>("git_checkout", { cwd, name, remote: remote ?? null });
}

export function gitCreateBranch(cwd: string, name: string): Promise<string> {
  return invoke<string>("git_create_branch", { cwd, name });
}

const GIT_CHANGED = "monocode-git-changed";

/** Tell git UIs (diff pane, branch picker) to reload after a local git mutation. */
export function notifyGitChanged() {
  window.dispatchEvent(new Event(GIT_CHANGED));
}

export function subscribeGitChanged(listener: () => void): () => void {
  window.addEventListener(GIT_CHANGED, listener);
  return () => window.removeEventListener(GIT_CHANGED, listener);
}

export function createPath(
  parent: string,
  name: string,
  isDir: boolean,
): Promise<string> {
  return invoke<string>("create_path", { parent, name, isDir });
}

export function renamePath(path: string, name: string): Promise<string> {
  return invoke<string>("rename_path", { path, name });
}

export function deletePath(path: string): Promise<void> {
  return invoke<void>("delete_path", { path });
}

export function copyPath(from: string, destParent: string): Promise<string> {
  return invoke<string>("copy_path", { from, destParent });
}

export function movePath(from: string, destParent: string): Promise<string> {
  return invoke<string>("move_path", { from, destParent });
}

export function revealPath(path: string): Promise<void> {
  return invoke<void>("reveal_path", { path });
}

export function homeDir(): Promise<string> {
  return invoke<string>("home_dir");
}

export async function pickFolder(title = "Open project"): Promise<string | null> {
  const selected = await open({
    directory: true,
    multiple: false,
    title,
  });
  return typeof selected === "string" && selected ? selected : null;
}

export async function pickFiles(title = "Attach files"): Promise<string[] | null> {
  const selected = await open({
    multiple: true,
    directory: false,
    title,
  });
  if (Array.isArray(selected)) {
    const paths = selected.filter((path): path is string => Boolean(path));
    return paths.length > 0 ? paths : null;
  }
  if (typeof selected === "string" && selected) return [selected];
  return null;
}

export function cloneRepo(url: string, parent: string): Promise<string> {
  return invoke<string>("clone_repo", { url, parent });
}

export function readFilePreview(
  path: string,
  maxLines = 6,
  startLine?: number,
): Promise<string[]> {
  return invoke<string[]>("read_file_preview", {
    path,
    maxLines,
    startLine,
  });
}

export type FileMtime = {
  path: string;
  mtimeMs: number | null;
};

export function statFiles(paths: string[]): Promise<FileMtime[]> {
  if (paths.length === 0) return Promise.resolve([]);
  return invoke<FileMtime[]>("stat_files", { paths });
}

export function readTextFile(path: string): Promise<string> {
  return invoke<string>("read_text_file", { path });
}

export function writeTextFile(path: string, content: string): Promise<void> {
  return invoke<void>("write_text_file", { path, content });
}

/** Last path segment, or `/` for the filesystem root. */
export function basename(path: string): string {
  const trimmed = path.replace(/\/+$/, "") || "/";
  const parts = trimmed.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? trimmed;
}

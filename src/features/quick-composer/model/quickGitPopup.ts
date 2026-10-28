import type { QuickWorkspace } from "./quickWorkspace";
import type { GitBranches } from "../../../platform/tauri/fs";

export const QUICK_GIT_REQUEST = "quick_git_request";
export const QUICK_GIT_RESULT = "quick_git_result";
export type QuickGitKind = "workspace" | "base" | "branch";
export type QuickGitRequest = {
  id: string;
  kind: QuickGitKind;
  choice: QuickWorkspace;
  branches?: GitBranches;
  anchor: { x: number; y: number; width: number; height: number };
};
export type QuickGitResult = {
  id: string;
  choice?: QuickWorkspace;
  restoreFocus: boolean;
  triggerKind?: QuickGitKind | null;
};

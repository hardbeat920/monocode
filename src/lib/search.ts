import { invoke } from "@tauri-apps/api/core";
import { pathKey, slash } from "./paths";

export type ProjectSearchMatch = {
  path: string;
  relative: string;
  line: number;
  column: number;
  preview: string;
};

export type ProjectSearchResult = {
  matches: ProjectSearchMatch[];
  truncated: boolean;
};

export type ProjectSearchOptions = {
  cwd: string;
  query: string;
  caseSensitive?: boolean;
  wholeWord?: boolean;
  regex?: boolean;
  include?: string;
  exclude?: string;
};

export type EditorNavigation = {
  line: number;
  column?: number;
};

export type EditorNavigationTarget = EditorNavigation & {
  path: string;
  token: number;
};

export type OpenFileFn = (path: string, navigation?: EditorNavigation) => void;

export function normalizeEditorPath(path: string): string {
  return slash(path).replace(/\/+$/, "") || path;
}

export function editorPathsEqual(a: string, b: string): boolean {
  return pathKey(a) === pathKey(b);
}

export function searchProject(
  options: ProjectSearchOptions,
): Promise<ProjectSearchResult> {
  return invoke<ProjectSearchResult>("search_project", { options });
}

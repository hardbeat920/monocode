import { basename, type GitChangedFile } from "../../../platform/tauri/fs";
import type { ChangesView } from "../../settings/model/appearance";
import { flattenVisible } from "../../../shared/lib/treeOrder";

export type ChangeDir = {
  name: string;
  /** Path relative to the repo root; "" for the implicit root. */
  path: string;
  dirs: ChangeDir[];
  files: GitChangedFile[];
  /** Status shared by every descendant, or null when they differ. */
  status: string | null;
};

/** Nests changed files under their directories, VS Code's tree view. */
export function buildChangeTree(files: readonly GitChangedFile[]): ChangeDir {
  const root: ChangeDir = {
    name: "",
    path: "",
    dirs: [],
    files: [],
    status: null,
  };
  for (const file of files) {
    const segments = file.relative.split("/");
    let node = root;
    for (const segment of segments.slice(0, -1)) {
      const path = node.path ? `${node.path}/${segment}` : segment;
      let next = node.dirs.find((dir) => dir.path === path);
      if (!next) {
        next = { name: segment, path, dirs: [], files: [], status: null };
        node.dirs.push(next);
      }
      node = next;
    }
    node.files.push(file);
  }
  sortChangeDir(root);
  return root;
}

/** Sorts each level (folders first) and rolls descendant status upward. */
function sortChangeDir(dir: ChangeDir): string | null {
  dir.dirs.sort((a, b) => a.name.localeCompare(b.name));
  dir.files.sort((a, b) =>
    basename(a.relative).localeCompare(basename(b.relative)),
  );
  let status: string | null = null;
  let mixed = false;
  const merge = (next: string | null) => {
    if (next === null) mixed = true;
    else if (status === null) status = next;
    else if (status !== next) mixed = true;
  };
  for (const child of dir.dirs) merge(sortChangeDir(child));
  for (const file of dir.files) merge(file.status);
  dir.status = mixed ? null : status;
  return dir.status;
}

/** Visible files of one section in on-screen order (repo-relative paths). */
export function visibleChangeOrder(
  files: readonly GitChangedFile[],
  view: ChangesView,
  isCollapsed: (dirPath: string) => boolean,
): string[] {
  if (view === "list") return files.map((file) => file.relative);
  const rowsOf = (dir: ChangeDir): (ChangeDir | GitChangedFile)[] => [
    ...dir.dirs,
    ...dir.files,
  ];
  return flattenVisible(rowsOf(buildChangeTree(files)), {
    children: (row) =>
      "dirs" in row && !isCollapsed(row.path) ? rowsOf(row) : null,
    id: (row) => ("dirs" in row ? row.path : row.relative),
    include: (row) => !("dirs" in row),
  });
}

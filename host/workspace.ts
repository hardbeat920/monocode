import { execFile } from "node:child_process";
import { promisify } from "node:util";
import {
  lstat,
  mkdir,
  open,
  readFile,
  readdir,
  realpath,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import { isAbsolute, relative, resolve, sep } from "node:path";
import {
  execGit,
  spawnGitWithInput,
} from "./git-exec";
import type {
  GitChangedFile,
  GitDiffIndex,
  GitFileDiff,
} from "../src/platform/tauri/fs";
import type {
  ProjectSearchMatch,
  ProjectSearchResult,
} from "../src/features/search/model/search";

const exec = promisify(execFile);
const MAX_FILE = 1024 * 1024;

export function workspacePath(
  root: string,
  input: unknown,
  allowRoot = false,
): string {
  if (typeof input !== "string" || input.length > 4096 || input.includes("\0"))
    throw new Error("Invalid workspace path");
  const path = resolve(root, input);
  const rel = relative(root, path);
  if (
    (!allowRoot && !rel) ||
    rel === ".." ||
    rel.startsWith(`..${sep}`) ||
    isAbsolute(rel) ||
    rel.split(sep).some((part) => part.toLowerCase() === ".git")
  )
    throw new Error("Path is outside the workspace");
  return path;
}

export async function existingPath(root: string, input: unknown, allowRoot = false) {
  const path = workspacePath(root, input, allowRoot);
  const actual = await realpath(path);
  const rel = relative(root, actual);
  if (
    (!allowRoot && !rel) ||
    rel === ".." ||
    rel.startsWith(`..${sep}`) ||
    isAbsolute(rel) ||
    rel.split(sep).some((part) => part.toLowerCase() === ".git")
  )
    throw new Error("Path is outside the workspace");
  return actual;
}

async function git(
  root: string,
  args: string[],
  maxBuffer = 4 * 1024 * 1024,
  readOnly = true,
) {
  return execGit(root, args, { maxBuffer, readOnly });
}

export type HostFileEntry = {
  name: string;
  path: string;
  isDir: boolean;
  ignored: boolean;
};

export async function listHostFiles(
  root: string,
  input: unknown,
): Promise<HostFileEntry[]> {
  const dir = await existingPath(root, input, true);
  if (!(await stat(dir)).isDirectory())
    throw new Error("Path is not a directory");
  const entries = await readdir(dir, { withFileTypes: true });
  if (entries.length > 5000)
    throw new Error("Directory has too many entries to display");
  const visible = entries.filter((entry) => entry.name !== ".DS_Store");
  const paths = visible.map((entry) =>
    relative(root, resolve(dir, entry.name)).split(sep).join("/"),
  );
  let ignored = new Set<string>();
  if (paths.length) {
    try {
      const output = await git(root, ["check-ignore", "-z", "--", ...paths]);
      ignored = new Set(output.split("\0").filter(Boolean));
    } catch {
      /* no ignored entries or a non-Git folder */
    }
  }
  const out = await Promise.all(
    visible.map(async (entry, index) => {
      const path = paths[index];
      const isDir =
        entry.isDirectory() ||
        (entry.isSymbolicLink() &&
          (await existingPath(root, path)
            .then((actual) => stat(actual))
            .then((value) => value.isDirectory())
            .catch(() => false)));
      return {
        name: entry.name,
        path,
        isDir,
        ignored: entry.name === ".git" || ignored.has(path),
      };
    }),
  );
  return out.sort(
    (a, b) =>
      Number(b.isDir) - Number(a.isDir) ||
      a.name.localeCompare(b.name, undefined, {
        numeric: true,
        sensitivity: "base",
      }),
  );
}

/** Upper bound on paths sent for Go to File, as for a local project. */
const MAX_INDEXED_FILES = 20_000;

/** Every file in the worktree, honoring .gitignore when it is a repository. */
async function hostFilePaths(root: string): Promise<string[]> {
  let paths: string[];
  let repository = false;
  try {
    repository = (await git(root, ["rev-parse", "--is-inside-work-tree"])).trim() === "true";
  } catch (error) {
    if (!String((error as { stderr?: string }).stderr).includes("not a git repository"))
      throw error;
  }
  if (repository) {
    paths = (
      await git(
        root,
        ["ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        8 * 1024 * 1024,
      )
    )
      .split("\0")
      .filter(Boolean);
  } else {
    paths = [];
    const pending = [""];
    while (pending.length && paths.length < 5000) {
      const dir = pending.pop()!;
      const entries = await readdir(resolve(root, dir), {
        withFileTypes: true,
      }).catch(() => []);
      for (const entry of entries) {
        if (entry.name === ".git" || entry.name === "node_modules") continue;
        const path = dir ? `${dir}/${entry.name}` : entry.name;
        if (entry.isDirectory()) pending.push(path);
        else if (entry.isFile()) paths.push(path);
      }
    }
  }
  return paths;
}

/** Relative paths of the worktree's files, for fuzzy Go to File on the client. */
export async function indexHostFiles(root: string): Promise<string[]> {
  return (await hostFilePaths(root)).slice(0, MAX_INDEXED_FILES);
}

/** Bounded filename search for the remote Explorer. */
export async function searchHostFiles(
  root: string,
  input: unknown,
): Promise<HostFileEntry[]> {
  if (typeof input !== "string" || input.length > 200)
    throw new Error("Invalid search");
  const query = input.trim().toLocaleLowerCase();
  if (!query) return [];
  return (await hostFilePaths(root))
    .filter((path) => path.toLocaleLowerCase().includes(query))
    .slice(0, 200)
    .map((path) => ({
      name: path.split("/").pop() || path,
      path,
      isDir: false,
      ignored: false,
    }));
}

type ContentSearchOptions = {
  query: string;
  caseSensitive: boolean;
  wholeWord: boolean;
  regex: boolean;
  include: string[];
  exclude: string[];
};

function searchOptions(input: unknown): ContentSearchOptions {
  if (!input || typeof input !== "object" || Array.isArray(input))
    throw new Error("Invalid search");
  const value = input as Record<string, unknown>;
  if (
    typeof value.query !== "string" ||
    value.query.length > 200 ||
    value.query.includes("\0") ||
    [value.include, value.exclude].some(
      (item) =>
        item !== undefined &&
        (typeof item !== "string" || item.length > 1000 || item.includes("\0")),
    )
  )
    throw new Error("Invalid search");
  for (const key of ["caseSensitive", "wholeWord", "regex"])
    if (value[key] !== undefined && typeof value[key] !== "boolean")
      throw new Error("Invalid search");
  const tokens = (item: unknown) =>
    (typeof item === "string" ? item : "")
      .split(",")
      .map((part) => part.trim())
      .filter(Boolean);
  return {
    query: value.query.trim(),
    caseSensitive: value.caseSensitive === true,
    wholeWord: value.wholeWord === true,
    regex: value.regex === true,
    include: tokens(value.include),
    exclude: tokens(value.exclude),
  };
}

function searchColumn(line: string, options: ContentSearchOptions): number {
  if (options.regex) return 1;
  const pattern = new RegExp(
    options.query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"),
    options.caseSensitive ? "g" : "gi",
  );
  for (const match of line.matchAll(pattern)) {
    if (options.wholeWord) {
      const before = line[match.index - 1];
      const after = line[match.index + match[0].length];
      if (
        (before && /[\p{L}\p{N}_]/u.test(before)) ||
        (after && /[\p{L}\p{N}_]/u.test(after))
      )
        continue;
    }
    return match.index + 1;
  }
  return 0;
}

/** Search file contents with the same controls and result shape as local search. */
export async function searchHostContent(
  root: string,
  input: unknown,
): Promise<ProjectSearchResult> {
  const options = searchOptions(input);
  if (!options.query) return { matches: [], truncated: false };
  const args = ["grep", "-z", "-n"];
  if (!options.caseSensitive) args.push("-i");
  if (options.wholeWord) args.push("-w");
  args.push(options.regex ? "-E" : "-F", "-e", options.query, "--");
  args.push(
    ...options.include,
    ...options.exclude.map((glob) => `:(exclude)${glob}`),
  );
  try {
    const output = await git(root, args, 8 * 1024 * 1024);
    const matches: ProjectSearchMatch[] = [];
    let offset = 0;
    while (offset < output.length && matches.length < 501) {
      const pathEnd = output.indexOf("\0", offset);
      const lineEnd = output.indexOf("\0", pathEnd + 1);
      const previewEnd = output.indexOf("\n", lineEnd + 1);
      if (pathEnd < 0 || lineEnd < 0) break;
      const relative = output.slice(offset, pathEnd).replace(/\\/g, "/");
      const preview = output.slice(
        lineEnd + 1,
        previewEnd < 0 ? undefined : previewEnd,
      );
      offset = previewEnd < 0 ? output.length : previewEnd + 1;
      if (
        !relative ||
        !Number.isFinite(Number(output.slice(pathEnd + 1, lineEnd)))
      )
        continue;
      const path = workspacePath(root, relative);
      matches.push({
        path: path.split(sep).join("/"),
        relative,
        line: Number(output.slice(pathEnd + 1, lineEnd)),
        column: searchColumn(preview, options) || 1,
        preview,
      });
    }
    return { matches: matches.slice(0, 500), truncated: matches.length > 500 };
  } catch (reason) {
    const code = (reason as { code?: number | string }).code;
    if (code === 1 || code === "1") return { matches: [], truncated: false };
    if (
      !/not a git repository|not in a git directory|maxBuffer/i.test(
        String(reason),
      )
    )
      throw reason;
  }

  // Ordinary folders use a bounded text scan, like the local fallback.
  if (options.regex) return { matches: [], truncated: false };
  const matches: ProjectSearchMatch[] = [];
  const pending = [""];
  let files = 0;
  let truncated = false;
  while (pending.length && matches.length <= 500 && files < 5000) {
    const parent = pending.pop()!;
    const entries = await readdir(resolve(root, parent), {
      withFileTypes: true,
    }).catch(() => []);
    for (const entry of entries) {
      if (entry.name === ".git" || entry.name === "node_modules") continue;
      const relative = parent ? `${parent}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        pending.push(relative);
        continue;
      }
      if (!entry.isFile()) continue;
      files++;
      if (files > 5000) {
        truncated = true;
        break;
      }
      if (!matchesFilters(relative, options)) continue;
      let content: string;
      let path: string;
      try {
        path = await existingPath(root, relative);
        if ((await stat(path)).size > 512 * 1024) continue;
        content = await readHostFile(root, relative);
      } catch {
        continue;
      }
      for (const [index, line] of content.split(/\r?\n/).entries()) {
        const column = searchColumn(line, options);
        if (!column) continue;
        matches.push({
          path: path.split(sep).join("/"),
          relative,
          line: index + 1,
          column,
          preview: line,
        });
        if (matches.length > 500) {
          truncated = true;
          break;
        }
      }
      if (matches.length > 500) break;
    }
  }
  if (files >= 5000 && pending.length) truncated = true;
  return { matches: matches.slice(0, 500), truncated };
}

function matchesFilters(path: string, options: ContentSearchOptions): boolean {
  const matches = (glob: string) => {
    const normalized = glob.replace(/^\.\//, "");
    if (!/[?*]/.test(normalized))
      return path === normalized || path.startsWith(`${normalized}/`);
    const pattern = normalized
      .replace(/[.+^${}()|[\]\\]/g, "\\$&")
      .replace(/\*\*/g, "\u0000")
      .replace(/\*/g, "[^/]*")
      .replace(/\?/g, "[^/]")
      .replace(/\u0000/g, ".*");
    const regex = new RegExp(`^${pattern}$`);
    return (
      regex.test(path) ||
      (!normalized.includes("/") && regex.test(path.split("/").pop()!))
    );
  };
  return (
    (!options.include.length || options.include.some(matches)) &&
    !options.exclude.some(matches)
  );
}

export async function readHostFile(
  root: string,
  input: unknown,
): Promise<string> {
  const path = await existingPath(root, input);
  const info = await stat(path);
  if (!info.isFile()) throw new Error("Path is not a file");
  if (info.size > MAX_FILE) throw new Error("File is too large to preview");
  const bytes = await readFile(path);
  if (bytes.includes(0)) throw new Error("Binary file cannot be previewed");
  return bytes.toString("utf8");
}

export async function writeHostFile(
  root: string,
  input: unknown,
  expected: unknown,
  content: unknown,
) {
  if (
    typeof expected !== "string" ||
    typeof content !== "string" ||
    Buffer.byteLength(content, "utf8") > MAX_FILE ||
    content.includes("\0")
  )
    throw new Error("Invalid file content");
  const path = await existingPath(root, input);
  const current = await readHostFile(root, input);
  if (current !== expected)
    throw new Error("File changed on the host; reload before saving");
  await writeFile(path, content, "utf8");
  invalidateGitIndexCache(root);
}

/** Create a new file or folder under an existing workspace directory. */
export async function createHostPath(
  root: string,
  parent: unknown,
  name: unknown,
  isDir: unknown,
): Promise<string> {
  if (
    typeof name !== "string" ||
    typeof isDir !== "boolean" ||
    !name ||
    name.length > 4096 ||
    name.includes("\0") ||
    /^[\\/]/.test(name)
  )
    throw new Error("Invalid file name");
  const parts = name.replace(/[\\/]+$/, "").split(/[\\/]/);
  if (
    !parts.length ||
    parts.some(
      (part) =>
        !part ||
        part === "." ||
        part === ".." ||
        part.toLowerCase() === ".git" ||
        part.length > 255 ||
        /^\s+$/.test(part),
    )
  )
    throw new Error("Invalid file name");
  let directory = await existingPath(root, parent, true);
  if (!(await stat(directory)).isDirectory())
    throw new Error("Path is not a directory");
  for (const part of parts.slice(0, -1)) {
    const candidate = workspacePath(
      root,
      relative(root, resolve(directory, part)),
    );
    await mkdir(candidate).catch((reason: NodeJS.ErrnoException) => {
      if (reason.code !== "EEXIST") throw reason;
    });
    directory = await existingPath(root, relative(root, candidate));
    if (!(await stat(directory)).isDirectory())
      throw new Error("Path is not a directory");
  }
  const path = workspacePath(
    root,
    relative(root, resolve(directory, parts.at(-1)!)),
  );
  if (isDir) await mkdir(path);
  else {
    const file = await open(path, "wx");
    await file.close();
  }
  invalidateGitIndexCache(root);
  return relative(root, path).split(sep).join("/");
}

function statusName(code: string): string {
  if (code === "??") return "untracked";
  if (code.includes("D")) return "deleted";
  if (code.includes("A") || code.includes("?")) return "added";
  return "modified";
}

const GIT_INDEX_TTL_MS = 3_000;
const GIT_META_TTL_MS = 60_000;
const CACHE_LIMIT = 64;
const MAX_UNTRACKED_EXPAND = 200;
const MAX_UNTRACKED_LINE_COUNT_FILES = 200;
const MAX_FINGERPRINT_FILES = 200;
const IGNORED_DIRS = new Set([
  "node_modules",
  ".git",
  "target",
  ".next",
  ".turbo",
  ".cache",
  "__pycache__",
  ".venv",
  "DerivedData",
]);

const emptyGitIndex = (): GitDiffIndex => ({
  branch: null,
  head: null,
  files: [],
  additions: 0,
  deletions: 0,
  remote: null,
  upstream: null,
  defaultBranch: null,
  ahead: 0,
  behind: 0,
  aheadOfDefault: 0,
  headPushed: false,
});

export type GitIndexOptions = {
  /** Resolve remote/default-branch metadata too (default true). */
  includeSync?: boolean;
  /** Skip the TTL cache and singleflight, e.g. before a destructive action. */
  bypassCache?: boolean;
};

type IndexEntry = {
  at: number;
  fingerprint: string;
  full: boolean;
  value: GitDiffIndex;
};
const gitIndexCache = new Map<string, IndexEntry>();
const inFlightGitIndex = new Map<
  string,
  { full: boolean; promise: Promise<GitDiffIndex> }
>();

type GitMeta = {
  remote: string | null;
  defaultBranch: string | null;
  headPushed: boolean;
  aheadOfDefault: number;
  fallbackAhead: number;
  fallbackBehind: number;
};
const gitMetaCache = new Map<
  string,
  { at: number; fingerprint: string; value: GitMeta }
>();

const overlaps = (a: string, b: string) =>
  a === b || a.startsWith(b + sep) || b.startsWith(a + sep);

/**
 * Forget cached Git snapshots for `root` (all roots when omitted). Call after
 * anything that writes to the repository or working tree from the host.
 */
export function invalidateGitIndexCache(root?: string) {
  writeEpoch++;
  for (const cache of [gitIndexCache, gitMetaCache])
    for (const key of [...cache.keys()])
      if (!root || overlaps(key.split("\0")[0], root)) cache.delete(key);
  for (const key of [...inFlightGitIndex.keys()])
    if (!root || overlaps(key, root)) inFlightGitIndex.delete(key);
}

/** Bumped by every write so snapshots started before it are never cached. */
let writeEpoch = 0;

function remember<T>(cache: Map<string, T>, key: string, value: T) {
  cache.delete(key);
  cache.set(key, value);
  while (cache.size > CACHE_LIMIT)
    cache.delete(cache.keys().next().value as string);
}

async function statSignature(path: string): Promise<string> {
  try {
    const info = await stat(path);
    return `${info.mtimeMs}:${info.size}`;
  } catch {
    return "-";
  }
}

/** Locate `.git` and the shared common dir without spawning git. */
async function resolveGitDirs(
  root: string,
): Promise<{ gitDir: string; commonDir: string } | null> {
  let dir = root;
  for (let depth = 0; depth < 64; depth++) {
    const dotGit = resolve(dir, ".git");
    const info = await stat(dotGit).catch(() => null);
    if (info?.isDirectory()) return { gitDir: dotGit, commonDir: dotGit };
    if (info?.isFile()) {
      const text = await readFile(dotGit, "utf8").catch(() => "");
      const target = /^gitdir:\s*(.+)$/m.exec(text)?.[1]?.trim();
      if (!target) return null;
      const gitDir = resolve(dir, target);
      const common = (
        await readFile(resolve(gitDir, "commondir"), "utf8").catch(() => "")
      ).trim();
      return { gitDir, commonDir: common ? resolve(gitDir, common) : gitDir };
    }
    const parent = resolve(dir, "..");
    if (parent === dir) return null;
    dir = parent;
  }
  return null;
}

/** Cheap stat-based proof that no Git state moved behind our back. */
async function gitStateFingerprint(root: string): Promise<string> {
  const dirs = await resolveGitDirs(root);
  const parts = await Promise.all([
    statSignature(root),
    ...(dirs
      ? [
          statSignature(resolve(dirs.gitDir, "HEAD")),
          statSignature(resolve(dirs.gitDir, "index")),
          statSignature(resolve(dirs.gitDir, "logs/HEAD")),
          statSignature(resolve(dirs.commonDir, "config")),
          statSignature(resolve(dirs.commonDir, "packed-refs")),
          statSignature(resolve(dirs.commonDir, "refs/remotes/origin")),
          statSignature(resolve(dirs.commonDir, "FETCH_HEAD")),
        ]
      : []),
  ]);
  return parts.join("|");
}

async function filesFingerprint(
  root: string,
  files: Pick<GitChangedFile, "relative">[],
): Promise<string> {
  const sigs = await Promise.all(
    files.slice(0, MAX_FINGERPRINT_FILES).map((file) =>
      lstat(resolve(root, file.relative), { bigint: true }).then(
        (info) => `${info.mtimeNs}:${info.ctimeNs}:${info.size}`,
        () => "-",
      ),
    ),
  );
  return sigs.join(",");
}

async function refsFingerprint(root: string): Promise<string> {
  const dirs = await resolveGitDirs(root);
  if (!dirs) return "";
  return (
    await Promise.all(
      ["config", "packed-refs", "refs/remotes", "refs/remotes/origin"].map(
        (name) => statSignature(resolve(dirs.commonDir, name)),
      ),
    )
  ).join("|");
}

function parseNumstat(text: string) {
  const counts = new Map<string, { additions: number; deletions: number }>();
  const tokens = text.split("\0");
  for (let index = 0; index < tokens.length; index++) {
    const match = /^(\d+|-)\t(\d+|-)\t([\s\S]*)$/.exec(tokens[index]);
    if (!match) continue;
    let path = match[3];
    if (!path) {
      // Rename/copy record: `add\tdel\t\0old\0new\0`.
      path = tokens[index + 2] ?? "";
      index += 2;
    }
    counts.set(path, {
      additions: Number(match[1]) || 0,
      deletions: Number(match[2]) || 0,
    });
  }
  return counts;
}

/** Individual files of a collapsed untracked directory, or null when too big. */
async function expandUntrackedDirectory(
  root: string,
  directory: string,
  budget: { left: number; dirsLeft: number },
): Promise<string[] | null> {
  const found: string[] = [];
  let left = budget.left;
  let dirsLeft = budget.dirsLeft;
  const pending = [directory.replace(/\/+$/, "")];
  while (pending.length) {
    if (--dirsLeft < 0) return null;
    const dir = pending.pop()!;
    const entries = await readdir(resolve(root, dir), {
      withFileTypes: true,
    }).catch(() => []);
    // A nested repository stays one collapsed entry, as in Git itself.
    if (entries.some((entry) => entry.name === ".git")) return null;
    const maybeIgnoredDirs: string[] = [];
    for (const entry of entries) {
      const path = `${dir}/${entry.name}`;
      if (entry.isDirectory()) {
        if (IGNORED_DIRS.has(entry.name)) maybeIgnoredDirs.push(path);
        else pending.push(path);
      } else {
        if (--left < 0) return null;
        found.push(path);
      }
    }
    if (maybeIgnoredDirs.length) {
      const ignored = await ignoredPaths(
        root,
        maybeIgnoredDirs.map((path) => `${path}/`),
      );
      // If `check-ignore` failed (e.g. timeout), keep the directory collapsed
      // rather than risking treating ignored files as untracked.
      if (!ignored) return null;
      for (const path of maybeIgnoredDirs) {
        if (!ignored.has(`${path}/`) && !ignored.has(path)) pending.push(path);
      }
    }
  }
  budget.left = left;
  budget.dirsLeft = dirsLeft;
  return found;
}

/**
 * Returns the subset of `paths` ignored by Git, or `null` if `git check-ignore`
 * failed for a reason other than exit code 1 ("no paths are ignored").
 */
async function ignoredPaths(
  root: string,
  paths: string[],
): Promise<Set<string> | null> {
  if (!paths.length) return new Set();
  try {
    const output = await spawnGitWithInput(
      root,
      ["check-ignore", "-z", "--stdin"],
      paths.join("\0") + "\0",
      true,
    );
    return new Set(output.split("\0").filter(Boolean));
  } catch (error) {
    if ((error as { code?: number | null })?.code === 1) return new Set();
    return null;
  }
}

async function computeGitIndex(
  root: string,
  full: boolean,
  options?: {
    strictIgnore?: boolean;
    onTrackedFingerprint?: (stable: boolean) => void;
  },
): Promise<GitDiffIndex> {
  let statusText: string;
  try {
    statusText = await execGit(
      root,
      [
        "status",
        "--porcelain=v2",
        "--branch",
        "-z",
        "--untracked-files=normal",
      ],
      { readOnly: true, maxBuffer: 16 * 1024 * 1024 },
    );
  } catch (error) {
    // An ordinary folder has an empty Changes view, just as it does locally.
    if (/not a git repository|must be run in a work tree/i.test(String(error)))
      return emptyGitIndex();
    throw error;
  }

  let head: string | null = null;
  let branch: string | null = null;
  let upstream: string | null = null;
  let ahead = 0;
  let behind = 0;
  let hasAheadBehind = false;
  const entries: {
    code: string;
    path: string;
    indexOid?: string;
  }[] = [];
  const tokens = statusText.split("\0");
  for (let index = 0; index < tokens.length; index++) {
    const item = tokens[index];
    if (!item) continue;
    if (item.startsWith("# ")) {
      const [key, ...rest] = item.slice(2).split(" ");
      const value = rest.join(" ");
      if (key === "branch.oid") head = value === "(initial)" ? null : value;
      else if (key === "branch.head")
        branch = value === "(detached)" ? null : value;
      else if (key === "branch.upstream") upstream = value || null;
      else if (key === "branch.ab") {
        const [a, b] = value.split(" ");
        ahead = Number(a) || 0;
        behind = Math.abs(Number(b)) || 0;
        hasAheadBehind = true;
      }
      continue;
    }
    const kind = item[0];
    let match: RegExpExecArray | null = null;
    if (kind === "1")
      match =
        /^1 (..) \S+ [0-7]{6} [0-7]{6} [0-7]{6} [0-9a-f]+ ([0-9a-f]+) ([\s\S]+)$/.exec(
          item,
        );
    else if (kind === "2") {
      match =
        /^2 (..) \S+ [0-7]{6} [0-7]{6} [0-7]{6} [0-9a-f]+ ([0-9a-f]+) [RC]\d+ ([\s\S]+)$/.exec(
          item,
        );
      index++; // original path of the rename
    }
    if (match) {
      entries.push({
        code: match[1].replace(/\./g, " "),
        indexOid: match[2],
        path: match[3],
      });
      continue;
    }
    const unmerged =
      kind === "u"
        ? /^u (..) \S+ [0-7]{6} [0-7]{6} [0-7]{6} [0-7]{6} [0-9a-f]+ [0-9a-f]+ [0-9a-f]+ ([\s\S]+)$/.exec(
            item,
          )
        : null;
    if (unmerged)
      entries.push({ code: unmerged[1].replace(/\./g, " "), path: unmerged[2] });
    else if (kind === "?" && item.length > 2)
      entries.push({ code: "??", path: item.slice(2) });
  }
  // A configured upstream that no longer exists reports no counts.
  if (!hasAheadBehind) upstream = null;

  const hasTrackedChanges = entries.some((entry) => entry.code !== "??");
  // Snapshot changed tracked files BEFORE numstat so concurrent edits during
  // diff computation are detected and never cached under a newer stat signature.
  const preTrackedFp = hasTrackedChanges
    ? await filesFingerprint(
        root,
        entries
          .filter((entry) => entry.code !== "??")
          .map((entry) => ({
            relative: entry.path,
            staged: entry.code[0] !== " ",
            unstaged: entry.code[1] !== " ",
            indexOid: entry.indexOid,
          })),
      )
    : "";
  const numstatPromise = hasTrackedChanges
    ? execGit(
        root,
        head
          ? [
              "-c",
              "core.quotepath=false",
              "diff",
              "--no-ext-diff",
              "--no-textconv",
              "--numstat",
              "-z",
              "HEAD",
              "--",
            ]
          : [
              // Before the first commit (`head === null`), diff staged index against the empty tree.
              "-c",
              "core.quotepath=false",
              "diff",
              "--cached",
              "--no-ext-diff",
              "--no-textconv",
              "--numstat",
              "-z",
              "--",
            ],
        { readOnly: true, maxBuffer: 16 * 1024 * 1024 },
      ).catch(() => "")
    : Promise.resolve("");
  const metaPromise =
    full && head
      ? gitMetadata(root, head, upstream, hasAheadBehind)
      : Promise.resolve(null);

  // Expand collapsed untracked directories so each new file is individually
  // listed and diffable, within bounded file and directory traversal budgets.
  const budget = {
    left: MAX_UNTRACKED_EXPAND,
    dirsLeft: MAX_UNTRACKED_EXPAND * 2,
  };
  const expanded = new Map<string, string[] | null>();
  const candidates: string[] = [];
  for (const entry of entries) {
    if (entry.code !== "??" || !entry.path.endsWith("/")) continue;
    const list = await expandUntrackedDirectory(root, entry.path, budget);
    expanded.set(entry.path, list);
    if (list) candidates.push(...list);
  }
  const ignored = await ignoredPaths(root, candidates);
  if (candidates.length && !ignored) {
    // Do not fall back to treating ignored files as untracked if check-ignore failed.
    if (options?.strictIgnore)
      throw new Error("Could not verify Git ignore rules for untracked files");
  }
  const untracked: string[] = [];
  const collapsed: string[] = [];
  for (const entry of entries) {
    if (entry.code !== "??") continue;
    if (!entry.path.endsWith("/")) untracked.push(entry.path);
    else {
      const list = ignored ? expanded.get(entry.path) : null;
      if (list) untracked.push(...list.filter((path) => !ignored!.has(path)));
      else collapsed.push(entry.path);
    }
  }

  const counts = parseNumstat(await numstatPromise);
  const files: GitChangedFile[] = [];
  for (const entry of entries) {
    if (entry.code === "??") continue;
    const count = counts.get(entry.path);
    files.push({
      path: entry.path,
      relative: entry.path,
      status: statusName(entry.code),
      additions: count?.additions ?? 0,
      deletions: count?.deletions ?? 0,
      staged: entry.code[0] !== " ",
      unstaged: entry.code[1] !== " ",
      ...(entry.indexOid ? { indexOid: entry.indexOid } : {}),
    });
  }
  let counted = 0;
  for (const path of [...untracked, ...collapsed]) {
    let additions = 0;
    if (!path.endsWith("/") && counted++ < MAX_UNTRACKED_LINE_COUNT_FILES) {
      try {
        const info = await lstat(resolve(root, path));
        if (info.size <= MAX_FILE && info.isFile()) {
          const content = await readFile(resolve(root, path), "utf8");
          additions = content
            ? content.split("\n").length - Number(content.endsWith("\n"))
            : 0;
        }
      } catch {
        /* a file may change during refresh */
      }
    }
    files.push({
      path,
      relative: path,
      status: "untracked",
      additions,
      deletions: 0,
      staged: false,
      unstaged: true,
    });
  }
  files.sort((a, b) => a.relative.localeCompare(b.relative));

  const meta = await metaPromise;
  const fallback = !upstream && meta?.defaultBranch;
  const result: GitDiffIndex = {
    branch,
    head,
    files,
    additions: files.reduce((sum, file) => sum + file.additions, 0),
    deletions: files.reduce((sum, file) => sum + file.deletions, 0),
    remote: meta?.remote ?? null,
    upstream,
    defaultBranch: meta?.defaultBranch ?? null,
    ahead: fallback ? meta!.fallbackAhead : ahead,
    behind: fallback ? meta!.fallbackBehind : behind,
    aheadOfDefault: meta?.aheadOfDefault ?? 0,
    headPushed: meta?.headPushed ?? false,
  };
  if (options?.onTrackedFingerprint) {
    const postTrackedFp = hasTrackedChanges
      ? await filesFingerprint(
          root,
          files.filter((file) => file.status !== "untracked"),
        )
      : "";
    options.onTrackedFingerprint(preTrackedFp === postTrackedFp);
  }
  return result;
}

/** Low-frequency remote/default-branch facts, cached by refs fingerprint. */
async function gitMetadata(
  root: string,
  head: string,
  upstream: string | null,
  hasAheadBehind: boolean,
): Promise<GitMeta> {
  const key = `${root}\0${head}\0${upstream ?? ""}\0${hasAheadBehind}`;
  const fingerprint = await refsFingerprint(root);
  const cached = gitMetaCache.get(key);
  if (
    cached &&
    cached.fingerprint === fingerprint &&
    Date.now() - cached.at < GIT_META_TTL_MS
  )
    return cached.value;
  const generation = writeEpoch;
  const [remoteText, refsText] = await Promise.all([
    execGit(root, ["remote", "get-url", "origin"]).catch(() => ""),
    execGit(root, [
      "for-each-ref",
      "--format=%(refname)%00%(symref)",
      "refs/remotes",
    ]).catch(() => ""),
  ]);
  const remote = remoteText.trim() ? "origin" : null;
  const refs = new Map<string, string>();
  for (const line of refsText.split("\n")) {
    const [name, symref] = line.split("\0");
    if (name) refs.set(name, symref ?? "");
  }
  let defaultBranch: string | null = null;
  const originHead = refs.get("refs/remotes/origin/HEAD");
  if (originHead?.startsWith("refs/remotes/origin/"))
    defaultBranch = originHead.slice("refs/remotes/origin/".length);
  if (!defaultBranch && remote)
    for (const name of ["main", "master"])
      if (refs.has(`refs/remotes/origin/${name}`)) {
        defaultBranch = name;
        break;
      }
  const [pushedText, countsText] = await Promise.all([
    refs.size
      ? execGit(root, [
          "for-each-ref",
          "--count=1",
          "--contains",
          head,
          "refs/remotes",
        ]).catch(() => "")
      : Promise.resolve(""),
    defaultBranch
      ? execGit(root, [
          "rev-list",
          "--left-right",
          "--count",
          `origin/${defaultBranch}...${head}`,
        ]).catch(() => "")
      : Promise.resolve(""),
  ]);
  const [defaultBehind, defaultAhead] = countsText
    .trim()
    .split(/\s+/)
    .map((value) => Number(value) || 0);
  const value: GitMeta = {
    remote,
    defaultBranch,
    headPushed: Boolean(pushedText.trim()),
    aheadOfDefault: defaultAhead || 0,
    fallbackAhead: defaultAhead || 0,
    fallbackBehind: defaultBehind || 0,
  };
  if (generation === writeEpoch)
    remember(gitMetaCache, key, { at: Date.now(), fingerprint, value });
  return value;
}

/**
 * Snapshot of the repository's changes. Concurrent calls share one run, and a
 * result stays valid for a few seconds unless Git state or a changed file's
 * stat moves, so frequent pollers cost no git processes while idle.
 */
export async function hostGitIndex(
  root: string,
  options: GitIndexOptions = {},
): Promise<GitDiffIndex> {
  const full = options.includeSync !== false;
  if (options.bypassCache) return computeGitIndex(root, full);

  const cached = gitIndexCache.get(root);
  if (
    cached &&
    (cached.full || !full) &&
    Date.now() - cached.at < GIT_INDEX_TTL_MS &&
    cached.fingerprint ===
      `${await gitStateFingerprint(root)}#${await filesFingerprint(root, cached.value.files)}`
  )
    return cached.value;

  const pending = inFlightGitIndex.get(root);
  if (pending && (pending.full || !full)) return pending.promise;

  const generation = writeEpoch;
  const promise = (async () => {
    const stateBefore = await gitStateFingerprint(root);
    let trackedStable = true;
    const value = await computeGitIndex(root, full, {
      onTrackedFingerprint: (stable) => {
        trackedStable = stable;
      },
    });
    const [stateAfter, filesFp] = await Promise.all([
      gitStateFingerprint(root),
      filesFingerprint(root, value.files),
    ]);
    if (
      generation === writeEpoch &&
      trackedStable &&
      stateBefore === stateAfter
    )
      remember(gitIndexCache, root, {
        at: Date.now(),
        fingerprint: `${stateAfter}#${filesFp}`,
        full,
        value,
      });
    return value;
  })();
  const flight = { full, promise };
  inFlightGitIndex.set(root, flight);
  try {
    return await promise;
  } finally {
    if (inFlightGitIndex.get(root) === flight) inFlightGitIndex.delete(root);
  }
}

/** File/addition/deletion totals without remote metadata. */
export async function hostGitStats(root: string) {
  const index = await hostGitIndex(root, { includeSync: false });
  return {
    files: index.files.length,
    additions: index.additions,
    deletions: index.deletions,
  };
}

/** Status of exactly one path (or a collapsed untracked folder), with a single targeted git process. */
async function scopedStatus(
  root: string,
  relativePath: string,
  options?: { unstagedOnly?: boolean; allowCollapsedUntrackedDir?: boolean },
) {
  const normalizedDir = relativePath.replace(/\/+$/, "");
  let text: string;
  try {
    text = await execGit(root, [
      "--literal-pathspecs",
      "status",
      "--porcelain=v1",
      "-z",
      "--untracked-files=all",
      "--",
      normalizedDir,
    ]);
  } catch (error) {
    if (/not a git repository|must be run in a work tree/i.test(String(error)))
      return undefined;
    throw error;
  }
  const parts = text.split("\0");
  let sawAny = false;
  let allUntrackedDescendants = true;
  const dirPrefix = `${normalizedDir}/`;
  for (let index = 0; index < parts.length; index++) {
    const item = parts[index];
    if (!item || item.length < 4) continue;
    const code = item.slice(0, 2);
    const path = item.slice(3);
    if (code.includes("R") || code.includes("C")) index++;
    const staged = code[0] !== " " && code !== "??";
    const unstaged = code[1] !== " " || code === "??";
    if (path === relativePath) {
      if (options?.unstagedOnly && !unstaged) continue;
      return {
        code,
        status: statusName(code),
        staged,
        unstaged,
      };
    }
    if (options?.allowCollapsedUntrackedDir && path.startsWith(dirPrefix)) {
      sawAny = true;
      if (code !== "??") allUntrackedDescendants = false;
    }
  }
  if (
    options?.allowCollapsedUntrackedDir &&
    relativePath.endsWith("/") &&
    sawAny &&
    allUntrackedDescendants
  ) {
    // Confirm the directory contains no tracked files before permitting recursive clean.
    const tracked = await execGit(root, [
      "--literal-pathspecs",
      "ls-files",
      "-z",
      "--",
      normalizedDir,
    ]).catch(() => "?");
    if (tracked === "") {
      return {
        code: "??",
        status: "untracked",
        staged: false,
        unstaged: true,
      };
    }
  }
  return undefined;
}

async function gitBlob(root: string, spec: string) {
  try {
    const text = await git(root, ["show", spec], MAX_FILE + 1024);
    return { text, binary: text.includes("\0"), tooLarge: false };
  } catch (error) {
    return {
      text: "",
      binary: false,
      tooLarge: String(error).includes("maxBuffer"),
    };
  }
}

const EMPTY_BLOB = { text: "", binary: false, tooLarge: false };

export async function hostFileDiff(
  root: string,
  input: unknown,
  staged: boolean,
): Promise<GitFileDiff> {
  const path = workspacePath(root, input);
  const relativePath = relative(root, path).split(sep).join("/");
  const file = await scopedStatus(root, relativePath);
  if (!file) throw new Error("File has no uncommitted changes");
  const original =
    (!staged && file.code === "??") || (staged && file.code[0] === "A")
      ? EMPTY_BLOB
      : await gitBlob(root, `${staged ? "HEAD" : ""}:${relativePath}`);
  const current =
    staged && file.code[0] === "D"
      ? EMPTY_BLOB
      : staged
        ? await gitBlob(root, `:${relativePath}`)
        : await readHostFile(root, relativePath).then(
            (text) => ({ text, binary: false, tooLarge: false }),
            (error) => {
              if (file.status === "deleted") return EMPTY_BLOB;
              if (String(error).includes("too large"))
                return { text: "", binary: false, tooLarge: true };
              if (String(error).includes("Binary file"))
                return { text: "", binary: true, tooLarge: false };
              throw error;
            },
          );
  return {
    path: relativePath,
    relative: relativePath,
    status: file.status,
    original: original.text,
    current: current.text,
    binary: original.binary || current.binary,
    tooLarge: original.tooLarge || current.tooLarge,
  };
}

export async function hostGitAction(
  root: string,
  action: unknown,
  input?: unknown,
  message?: unknown,
  contents?: unknown,
) {
  try {
    return await runGitAction(root, action, input, message, contents);
  } finally {
    invalidateGitIndexCache(root);
  }
}

async function runGitAction(
  root: string,
  action: unknown,
  input?: unknown,
  message?: unknown,
  contents?: unknown,
) {
  const discard = async (file: { relative: string; status: string }) => {
    const path = relative(root, workspacePath(root, file.relative));
    if (file.status === "untracked" && file.relative.endsWith("/"))
      await git(
        root,
        ["--literal-pathspecs", "clean", "-fd", "--", path],
        undefined,
        false,
      );
    else if (file.status === "untracked") await rm(resolve(root, path));
    else
      await git(
        root,
        ["--literal-pathspecs", "restore", "--worktree", "--", path],
        undefined,
        false,
      );
  };
  switch (action) {
    case "stageContents": {
      const path = relative(root, workspacePath(root, input)).split(sep).join("/");
      if (
        typeof contents !== "string" ||
        Buffer.byteLength(contents) > MAX_FILE
      )
        throw new Error("File too large or invalid");
      const hash = (
        await spawnGitWithInput(
          root,
          ["hash-object", "-w", "--path", path, "--stdin"],
          contents,
        )
      ).trim();
      const staged = await git(root, ["ls-files", "--stage", "--", path]);
      const mode = staged.match(/^([0-7]{6})\s/)?.[1] ?? "100644";
      await git(
        root,
        ["update-index", "--add", "--cacheinfo", mode, hash, path],
        undefined,
        false,
      );
      return;
    }
    case "stage":
    case "unstage": {
      const path = relative(root, workspacePath(root, input));
      await git(
        root,
        action === "stage"
          ? ["--literal-pathspecs", "add", "--", path]
          : ["--literal-pathspecs", "restore", "--staged", "--", path],
        undefined,
        false,
      );
      return;
    }
    case "stageAll":
      await git(root, ["add", "-A", "--", "."], undefined, false);
      return;
    case "unstageAll":
      await git(root, ["reset", "-q", "--", "."], undefined, false);
      return;
    case "discard": {
      const requestedDir = typeof input === "string" && input.endsWith("/");
      const normalized = relative(root, workspacePath(root, input))
        .split(sep)
        .join("/");
      const path = requestedDir ? `${normalized}/` : normalized;
      const file = await scopedStatus(root, path, {
        unstagedOnly: true,
        allowCollapsedUntrackedDir: requestedDir,
      });
      if (!file?.unstaged) throw new Error("File has no changes to discard");
      await discard({ relative: path, status: file.status });
      return;
    }
    case "discardAll":
      for (const file of (
        await computeGitIndex(root, false, { strictIgnore: true })
      ).files.filter((entry) => entry.unstaged))
        await discard(file);
      return;
    case "commit": {
      if (
        typeof message !== "string" ||
        !message.trim() ||
        message.length > 100_000
      )
        throw new Error("Enter a commit message");
      await git(root, ["commit", "-m", message], 1024 * 1024, false);
      return;
    }
    case "push":
      await git(root, ["push", "-u", "origin", "HEAD"], undefined, false);
      return;
    case "createPr": {
      const output = await exec("gh", ["pr", "create", "--fill"], {
        cwd: root,
        timeout: 30_000,
        maxBuffer: 1024 * 1024,
        env: {
          ...process.env,
          GH_PROMPT_DISABLED: "1",
          GIT_TERMINAL_PROMPT: "0",
        },
      });
      return output.stdout.trim();
    }
    default:
      throw new Error("Unsupported Git action");
  }
}

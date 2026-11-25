import { existsSync, realpathSync, statSync } from "node:fs";
import { mkdir, readdir, readFile, stat } from "node:fs/promises";
import { basename, dirname, join, resolve, sep } from "node:path";
import { execGit, execGitSync } from "./git-exec";
import { invalidateGitIndexCache } from "./workspace";

const WORKTREES_TTL_MS = 15_000;
const CACHE_LIMIT = 64;

function available(path: string): boolean {
  try {
    return statSync(path).isDirectory();
  } catch {
    return false;
  }
}

export type HostWorktree = {
  path: string;
  branch: string | null;
  head: string;
  isMain: boolean;
  missing: boolean;
};
export type HostWorktrees = { worktrees: HostWorktree[]; defaultRoot: string };

function parse(text: string): HostWorktree[] {
  const trees: HostWorktree[] = [];
  let tree: HostWorktree | undefined;
  for (const field of text.split("\0")) {
    if (field.startsWith("worktree ")) {
      tree = {
        path: field.slice(9),
        branch: null,
        head: "",
        isMain: trees.length === 0,
        missing: false,
      };
      trees.push(tree);
    } else if (tree && field.startsWith("HEAD ")) {
      tree.head = field.slice(5);
    } else if (tree && field.startsWith("branch refs/heads/")) {
      tree.branch = field.slice("branch refs/heads/".length);
    }
  }
  return trees;
}

function registeredSync(cwd: string): HostWorktree[] {
  const output = execGitSync(cwd, ["worktree", "list", "--porcelain", "-z"], {
    readOnly: true,
    maxBuffer: 1024 * 1024,
  });
  return parse(output).map((tree) => ({
    ...tree,
    path: available(tree.path) ? realpathSync.native(tree.path) : tree.path,
  }));
}

export function resolveHostWorktree(
  projectCwd: string,
  requested: unknown,
): string {
  if (requested == null || requested === "" || requested === projectCwd)
    return projectCwd;
  if (
    typeof requested !== "string" ||
    requested.includes("\0") ||
    requested.length > 4096
  )
    throw new Error("Invalid working copy");
  const actual = available(requested) ? realpathSync.native(requested) : requested;
  if (actual === projectCwd) return projectCwd;
  const target = registeredSync(projectCwd).find(
    (tree) => tree.path === actual,
  );
  if (!target || !available(target.path))
    throw new Error("Choose an available worktree of this project");
  return realpathSync.native(target.path);
}

export async function resolveHostWorktreeAsync(
  projectCwd: string,
  requested: unknown,
): Promise<string> {
  if (requested == null || requested === "" || requested === projectCwd)
    return projectCwd;
  if (
    typeof requested !== "string" ||
    requested.includes("\0") ||
    requested.length > 4096
  )
    throw new Error("Invalid working copy");
  const actual = available(requested) ? realpathSync.native(requested) : requested;
  if (actual === projectCwd) return projectCwd;
  const listed = await hostWorktrees(projectCwd);
  if (
    !listed.worktrees.some((tree) => tree.path === actual && !tree.missing) ||
    !available(actual)
  )
    throw new Error("Choose an available worktree of this project");
  return actual;
}

type WorktreesEntry = { at: number; fingerprint: string; value: HostWorktrees };
const worktreesCache = new Map<string, WorktreesEntry>();
const worktreesInFlight = new Map<string, Promise<HostWorktrees>>();
let worktreesEpoch = 0;

/** Forget cached worktree lists for `cwd` (all projects when omitted). */
export function invalidateWorktreesCache(cwd?: string) {
  worktreesEpoch++;
  for (const key of [...worktreesCache.keys()])
    if (!cwd || key === cwd || key.startsWith(cwd + sep) || cwd.startsWith(key + sep))
      worktreesCache.delete(key);
  for (const key of [...worktreesInFlight.keys()])
    if (!cwd || key === cwd || key.startsWith(cwd + sep) || cwd.startsWith(key + sep))
      worktreesInFlight.delete(key);
}

/** The `worktrees` directory of the shared git dir, found without git. */
async function worktreesFingerprint(cwd: string): Promise<string> {
  let dir = cwd;
  for (let depth = 0; depth < 64; depth++) {
    const dotGit = resolve(dir, ".git");
    const info = await stat(dotGit).catch(() => null);
    let common: string | undefined;
    if (info?.isDirectory()) common = dotGit;
    else if (info?.isFile()) {
      const text = await readFile(dotGit, "utf8").catch(() => "");
      const target = /^gitdir:\s*(.+)$/m.exec(text)?.[1]?.trim();
      if (!target) return "?";
      const gitDir = resolve(dir, target);
      const shared = (
        await readFile(resolve(gitDir, "commondir"), "utf8").catch(() => "")
      ).trim();
      common = shared ? resolve(gitDir, shared) : gitDir;
    }
    if (common) {
      const sig = (path: string) =>
        stat(path, { bigint: true }).then(
          (value) => `${value.mtimeNs}:${value.ctimeNs}:${value.size}`,
          () => "-",
        );
      const worktreesDir = resolve(common, "worktrees");
      const linkedNames = await readdir(worktreesDir).catch(() => []);
      const headFiles = [
        resolve(common, "HEAD"),
        ...linkedNames.sort().map((name) => resolve(worktreesDir, name, "HEAD")),
      ];
      // Resolve attached symbolic HEAD refs so commits on checked-out branches
      // invalidate the cached worktree head SHA immediately.
      const branchRefs = (
        await Promise.all(
          headFiles.map(async (headFile) => {
            const text = await readFile(headFile, "utf8").catch(() => "");
            const ref = /^ref:\s*(.+)$/m.exec(text)?.[1]?.trim();
            return ref ? resolve(common, ref) : null;
          }),
        )
      ).filter((ref): ref is string => Boolean(ref));
      const probes = [
        worktreesDir,
        resolve(common, "config"),
        resolve(common, "packed-refs"),
        ...headFiles,
        ...branchRefs,
      ];
      return (await Promise.all(probes.map(sig))).join("|");
    }
    const parent = resolve(dir, "..");
    if (parent === dir) break;
    dir = parent;
  }
  return "?";
}

/** Worktree list, shared between concurrent callers and cached for a few seconds. */
export async function hostWorktrees(cwd: string): Promise<HostWorktrees> {
  const cached = worktreesCache.get(cwd);
  if (cached && Date.now() - cached.at < WORKTREES_TTL_MS) {
    const fingerprint = await worktreesFingerprint(cwd);
    const availabilityUnchanged = cached.value.worktrees.every(
      (tree) => available(tree.path) === !tree.missing,
    );
    if (
      fingerprint !== "?" &&
      fingerprint === cached.fingerprint &&
      availabilityUnchanged
    )
      return cached.value;
  }
  const pending = worktreesInFlight.get(cwd);
  if (pending) return pending;
  const epoch = worktreesEpoch;
  const promise = (async () => {
    const fingerprint = await worktreesFingerprint(cwd);
    const value = await listWorktrees(cwd);
    if (epoch === worktreesEpoch) {
      worktreesCache.delete(cwd);
      worktreesCache.set(cwd, { at: Date.now(), fingerprint, value });
      while (worktreesCache.size > CACHE_LIMIT)
        worktreesCache.delete(worktreesCache.keys().next().value as string);
    }
    return value;
  })();
  worktreesInFlight.set(cwd, promise);
  try {
    return await promise;
  } finally {
    if (worktreesInFlight.get(cwd) === promise) worktreesInFlight.delete(cwd);
  }
}

async function listWorktrees(cwd: string): Promise<HostWorktrees> {
  const stdout = await execGit(cwd, ["worktree", "list", "--porcelain", "-z"], {
    readOnly: true,
    maxBuffer: 1024 * 1024,
  });
  const worktrees = parse(stdout).map((tree) => ({
    ...tree,
    path: available(tree.path) ? realpathSync.native(tree.path) : tree.path,
    missing: !available(tree.path),
  }));
  const main = worktrees[0];
  if (!main) throw new Error("No working copies found");
  return {
    worktrees,
    defaultRoot: join(dirname(main.path), `${basename(main.path)}-worktrees`),
  };
}

export async function createHostWorktree(
  cwd: string,
  branch: unknown,
  base: unknown,
  existing: unknown,
  sourceCwd = cwd,
): Promise<HostWorktree> {
  if (
    typeof branch !== "string" ||
    !branch ||
    branch.length > 120 ||
    branch.startsWith("-") ||
    branch.startsWith("@")
  )
    throw new Error("Enter a valid branch name");
  await execGit(cwd, ["check-ref-format", "--branch", branch], { readOnly: true });
  if (
    typeof base !== "string" ||
    base.length > 255 ||
    typeof existing !== "boolean"
  )
    throw new Error("Invalid worktree base");
  const listed = await hostWorktrees(cwd);
  if (listed.worktrees.some((tree) => tree.branch === branch))
    throw new Error(
      "This branch already has a working copy. Select it from the picker.",
    );
  const slug = `wt-${branch.replace(/[^a-zA-Z0-9_-]/g, "-")}`;
  const path = join(listed.defaultRoot, slug);
  if (existsSync(path))
    throw new Error(`${path} already exists. Choose another branch name.`);
  const refs = (
    await execGit(
      cwd,
      ["for-each-ref", "--format=%(refname)", "refs/heads", "refs/remotes"],
      { readOnly: true, maxBuffer: 1024 * 1024 },
    )
  )
    .split("\n")
    .filter(Boolean);
  const source = existing
    ? `refs/heads/${branch}`
    : base === "HEAD"
      ? "HEAD"
      : refs.find(
          (ref) =>
            ref === `refs/heads/${base}` || ref === `refs/remotes/${base}`,
        );
  if (!source || (existing && !refs.includes(source)))
    throw new Error("Choose an available base branch");
  const commit = (
    await execGit(
      sourceCwd,
      ["rev-parse", "--verify", "--end-of-options", `${source}^{commit}`],
      { readOnly: true, maxBuffer: 1024 * 1024 },
    )
  ).trim();
  await mkdir(listed.defaultRoot, { recursive: true });
  try {
    if (existing)
      await execGit(cwd, ["worktree", "add", "--", path, branch], {
        readOnly: false,
        maxBuffer: 1024 * 1024,
      });
    else
      await execGit(
        cwd,
        ["worktree", "add", "--no-track", "-b", branch, "--", path, commit],
        { readOnly: false, maxBuffer: 1024 * 1024 },
      );
  } finally {
    invalidateWorktreesCache(cwd);
    invalidateGitIndexCache(cwd);
  }
  const created = (await hostWorktrees(cwd)).worktrees.find(
    (tree) => tree.path === path,
  );
  if (!created)
    throw new Error(
      "Worktree created, but could not be found. Refresh the picker.",
    );
  return created;
}

/** Rename only the temporary branch created for this specific worktree. */
export async function renameHostWorktreeBranch(
  cwd: string,
  path: string,
  expectedBranch: string,
  branch: string,
  stillOwned: () => boolean = () => true,
): Promise<HostWorktree> {
  if (!/^mc\/[a-z0-9]{8}$/.test(expectedBranch))
    throw new Error("This is not an automatically created worktree branch");
  if (
    !branch ||
    branch.length > 120 ||
    branch.startsWith("-") ||
    branch.startsWith("@")
  )
    throw new Error("Enter a valid branch name");
  await execGit(cwd, ["check-ref-format", "--branch", branch], { readOnly: true });
  const tree = (await listWorktrees(cwd)).worktrees.find(
    (entry) => entry.path === path && !entry.missing,
  );
  if (!tree || tree.isMain || tree.branch !== expectedBranch)
    throw new Error("The worktree branch has changed");
  if (branch === expectedBranch) return tree;
  if (!stillOwned()) throw new Error("The session no longer owns this branch");
  try {
    await execGit(tree.path, ["branch", "-m", expectedBranch, branch], {
      readOnly: false,
      maxBuffer: 1024 * 1024,
    });
  } finally {
    invalidateWorktreesCache(cwd);
    invalidateGitIndexCache(tree.path);
  }
  const renamed = (await hostWorktrees(cwd)).worktrees.find(
    (entry) => entry.path === path,
  );
  if (!renamed)
    throw new Error("Branch renamed, but its worktree could not be found");
  return renamed;
}

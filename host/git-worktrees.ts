import { execFile, execFileSync } from "node:child_process";
import { existsSync, realpathSync, statSync } from "node:fs";
import { mkdir } from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import { promisify } from "node:util";

const exec = promisify(execFile);
const options = (cwd: string) => ({
  cwd,
  timeout: 10_000,
  maxBuffer: 1024 * 1024,
});

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
  const output = execFileSync(
    "git",
    ["worktree", "list", "--porcelain", "-z"],
    {
      ...options(cwd),
      encoding: "utf8",
    },
  );
  return parse(output).map((tree) => ({
    ...tree,
    path: available(tree.path) ? realpathSync(tree.path) : tree.path,
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
  const actual = available(requested) ? realpathSync(requested) : requested;
  if (actual === projectCwd) return projectCwd;
  const target = registeredSync(projectCwd).find(
    (tree) => tree.path === actual,
  );
  if (!target || !available(target.path))
    throw new Error("Choose an available worktree of this project");
  return realpathSync(target.path);
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
  const actual = available(requested) ? realpathSync(requested) : requested;
  if (actual === projectCwd) return projectCwd;
  const listed = await hostWorktrees(projectCwd);
  if (!listed.worktrees.some((tree) => tree.path === actual && !tree.missing))
    throw new Error("Choose an available worktree of this project");
  return actual;
}

export async function hostWorktrees(cwd: string): Promise<HostWorktrees> {
  const { stdout } = await exec(
    "git",
    ["worktree", "list", "--porcelain", "-z"],
    options(cwd),
  );
  const worktrees = parse(stdout).map((tree) => ({
    ...tree,
    path: available(tree.path) ? realpathSync(tree.path) : tree.path,
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
  await exec("git", ["check-ref-format", "--branch", branch], options(cwd));
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
    await exec(
      "git",
      ["for-each-ref", "--format=%(refname)", "refs/heads", "refs/remotes"],
      options(cwd),
    )
  ).stdout
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
    await exec(
      "git",
      ["rev-parse", "--verify", "--end-of-options", `${source}^{commit}`],
      options(sourceCwd),
    )
  ).stdout.trim();
  await mkdir(listed.defaultRoot, { recursive: true });
  if (existing)
    await exec("git", ["worktree", "add", "--", path, branch], options(cwd));
  else
    await exec(
      "git",
      ["worktree", "add", "--no-track", "-b", branch, "--", path, commit],
      options(cwd),
    );
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
  await exec("git", ["check-ref-format", "--branch", branch], options(cwd));
  const tree = (await hostWorktrees(cwd)).worktrees.find(
    (entry) => entry.path === path && !entry.missing,
  );
  if (!tree || tree.isMain || tree.branch !== expectedBranch)
    throw new Error("The worktree branch has changed");
  if (branch === expectedBranch) return tree;
  await exec(
    "git",
    ["branch", "-m", expectedBranch, branch],
    options(tree.path),
  );
  const renamed = (await hostWorktrees(cwd)).worktrees.find(
    (entry) => entry.path === path,
  );
  if (!renamed)
    throw new Error("Branch renamed, but its worktree could not be found");
  return renamed;
}

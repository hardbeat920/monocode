import { execGit } from "./git-exec";
import { invalidateWorktreesCache } from "./git-worktrees";
import { invalidateGitIndexCache } from "./workspace";

const read = (cwd: string, args: string[]) =>
  execGit(cwd, args, { readOnly: true, maxBuffer: 1024 * 1024 });
const write = (cwd: string, args: string[]) =>
  execGit(cwd, args, { readOnly: false, maxBuffer: 1024 * 1024 });

export type HostBranches = {
  current: string | null;
  branches: string[];
  remotes: { remote: string; name: string }[];
};

export async function hostBranches(cwd: string): Promise<HostBranches> {
  const [names, remoteNames, current] = await Promise.all([
    read(cwd, ["for-each-ref", "--format=%(refname:short)", "refs/heads"]),
    read(cwd, [
      "for-each-ref",
      "--format=%(refname:short)%00%(symref)",
      "refs/remotes",
    ]),
    read(cwd, ["symbolic-ref", "--quiet", "--short", "HEAD"])
      .then((stdout) => stdout.trim())
      .catch(() => null),
  ]);
  const remotes = remoteNames
    .split("\n")
    .filter(Boolean)
    .flatMap((line) => {
      const [ref, symref] = line.split("\0");
      const slash = ref.indexOf("/");
      return slash > 0 && !symref
        ? [{ remote: ref.slice(0, slash), name: ref.slice(slash + 1) }]
        : [];
    });
  return { current, branches: names.split("\n").filter(Boolean), remotes };
}

export async function switchHostBranch(
  cwd: string,
  branch: unknown,
  remote?: unknown,
): Promise<HostBranches> {
  if (
    typeof branch !== "string" ||
    branch.length > 255 ||
    !branch ||
    branch.startsWith("-")
  )
    throw new Error("Invalid branch");
  const state = await hostBranches(cwd);
  const remoteRef =
    typeof remote === "string" &&
    state.remotes.some(
      (entry) => entry.remote === remote && entry.name === branch,
    )
      ? `refs/remotes/${remote}/${branch}`
      : null;
  if (remote != null && !remoteRef)
    throw new Error("Choose an available remote branch");
  if (!state.branches.includes(branch) && !remoteRef)
    throw new Error("Choose an existing local branch");
  if (state.current === branch) return state;
  const changes = await read(cwd, [
    "status",
    "--porcelain",
    "--untracked-files=normal",
  ]);
  if (changes)
    throw new Error(
      "Commit or stash changes on the host before switching branches",
    );
  try {
    if (remoteRef && !state.branches.includes(branch))
      await write(cwd, ["switch", "--track", "-c", branch, remoteRef]);
    else await write(cwd, ["switch", branch]);
  } finally {
    invalidateGitIndexCache(cwd);
    invalidateWorktreesCache(cwd);
  }
  return hostBranches(cwd);
}

export async function createHostBranch(
  cwd: string,
  branch: unknown,
): Promise<HostBranches> {
  if (
    typeof branch !== "string" ||
    !branch ||
    branch.length > 255 ||
    branch.startsWith("-") ||
    branch.startsWith("@")
  )
    throw new Error("Enter a valid branch name");
  await read(cwd, ["check-ref-format", "--branch", branch]);
  const state = await hostBranches(cwd);
  if (state.branches.includes(branch)) throw new Error("Branch already exists");
  const changes = await read(cwd, [
    "status",
    "--porcelain",
    "--untracked-files=normal",
  ]);
  if (changes)
    throw new Error(
      "Commit or stash changes on the host before switching branches",
    );
  try {
    await write(cwd, ["switch", "-c", branch]);
  } finally {
    invalidateGitIndexCache(cwd);
    invalidateWorktreesCache(cwd);
  }
  return hostBranches(cwd);
}

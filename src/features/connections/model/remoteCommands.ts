import { setRemoteCommandRunner } from "../../../platform/tauri/fs";
import { remoteMachineFor, remoteRequest } from "./connections";
import { parseRemotePath, remotePath } from "./remoteProjects";

/** File commands a connected machine answers exactly as this computer does
 * (see host/workspace-commands.ts). */
const HOST_COMMANDS = new Set([
  "list_dir",
  "list_project_files",
  "read_text_file",
  "read_binary_file",
  "read_file_preview",
  "write_text_file",
  "stat_files",
  "create_path",
  "rename_path",
  "delete_path",
  "copy_path",
  "move_path",
  "git_diff_index",
  "git_diff_files",
  "git_diff_stats",
  "git_file_diff",
  "git_stage_contents",
  "git_stage_file",
  "git_unstage_file",
  "git_discard_file",
  "git_discard_all",
  "git_stage_all",
  "git_unstage_all",
  "git_commit",
  "git_head_message",
  "git_push",
  "git_pull",
  "git_sync",
  "git_pr_status",
  "git_pr_create",
  "git_history",
  "git_commit_files",
  "git_commit_file_diff",
  "git_staged_context",
  "git_range_context",
  "git_branches",
  "git_checkout",
  "git_create_branch",
  "git_stash",
  "git_worktrees",
  "search_project",
  "list_skills",
]);
/** Arguments that hold paths; everything else is passed through untouched. */
const PATH_ARGS = ["path", "cwd", "parent", "from", "destParent", "paths"];
/** Commands whose string result is a path. */
const PATH_RESULTS = new Set([
  "create_path",
  "rename_path",
  "copy_path",
  "move_path",
]);
/** Commands whose result entries carry a `path`. */
const ENTRY_RESULTS = new Set([
  "list_dir",
  "list_project_files",
  "stat_files",
  "list_skills",
]);

const UNAVAILABLE = "This isn’t available for projects on another machine yet.";
const OUTDATED =
  "Update MonoCode Host in Connections settings to use this project’s files.";

/** Runs a file command whose paths are `remote://` paths on the machine that
 * owns them, translating paths both ways so callers never see host paths. */
export async function runRemoteCommand(
  command: string,
  args: Record<string, unknown>,
): Promise<unknown> {
  if (!HOST_COMMANDS.has(command)) throw new Error(UNAVAILABLE);
  let environmentId: string | undefined;
  const toHost = (value: unknown): unknown => {
    if (Array.isArray(value)) return value.map(toHost);
    if (typeof value !== "string") return value;
    const parsed = parseRemotePath(value);
    if (!parsed || (environmentId && parsed.environmentId !== environmentId))
      throw new Error(
        "Files can only be copied or moved within one machine.",
      );
    environmentId = parsed.environmentId;
    return parsed.hostPath;
  };
  const hostArgs = Object.fromEntries(
    Object.entries(args).map(([key, value]) => [
      key,
      PATH_ARGS.includes(key)
        ? toHost(value)
        : key === "options" && value && typeof value === "object" && !Array.isArray(value)
          ? { ...value, cwd: toHost((value as Record<string, unknown>).cwd) }
          : value,
    ]),
  );
  if (!environmentId) throw new Error(UNAVAILABLE);
  const env = environmentId;
  const machine = await remoteMachineFor(env);
  if (!machine)
    throw new Error("This project’s machine isn’t connected on this computer.");
  let result: unknown;
  try {
    result = await remoteRequest(machine.id, "workspace.run", {
      command,
      args: hostArgs,
    });
  } catch (reason) {
    if (/Unsupported (host method|remote operation)/i.test(String(reason)))
      throw new Error(OUTDATED);
    throw reason;
  }
  const fromHost = (path: string) => remotePath(env, path);
  if (PATH_RESULTS.has(command) && typeof result === "string")
    return fromHost(result);
  if (ENTRY_RESULTS.has(command) && Array.isArray(result))
    return result.map((entry: { path: string }) => ({
      ...entry,
      path: fromHost(entry.path),
    }));
  if ((command === "git_diff_index" || command === "git_diff_files") && result && typeof result === "object") {
    const index = result as { files: { path: string }[] };
    const root = String(hostArgs.cwd).replace(/[\\/]+$/, "");
    return {
      ...index,
      files: index.files.map((file) => ({
        ...file,
        path: fromHost(`${root}/${file.path}`),
      })),
    };
  }
  if (command === "git_file_diff" && result && typeof result === "object") {
    const diff = result as { path: string };
    const root = String(hostArgs.cwd).replace(/[\\/]+$/, "");
    return { ...diff, path: fromHost(`${root}/${diff.path}`) };
  }
  if (command === "git_commit_files" && Array.isArray(result)) {
    const root = String(hostArgs.cwd).replace(/[\\/]+$/, "");
    return result.map((file: { path: string }) => ({
      ...file,
      path: fromHost(`${root}/${file.path}`),
    }));
  }
  if (command === "git_commit_file_diff" && result && typeof result === "object") {
    const diff = result as { path: string };
    const root = String(hostArgs.cwd).replace(/[\\/]+$/, "");
    return { ...diff, path: fromHost(`${root}/${diff.path}`) };
  }
  if (command === "search_project" && result && typeof result === "object") {
    const search = result as { matches: { path: string }[] };
    return {
      ...search,
      matches: search.matches.map((match) => ({ ...match, path: fromHost(match.path) })),
    };
  }
  if (command === "git_worktrees" && result && typeof result === "object") {
    const worktrees = result as { defaultRoot: string; worktrees: { path: string }[] };
    return {
      ...worktrees,
      defaultRoot: fromHost(worktrees.defaultRoot),
      worktrees: worktrees.worktrees.map((tree) => ({ ...tree, path: fromHost(tree.path) })),
    };
  }
  return result;
}

setRemoteCommandRunner(runRemoteCommand);

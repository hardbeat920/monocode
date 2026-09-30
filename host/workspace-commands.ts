import {
  cp,
  lstat,
  mkdir,
  readFile,
  realpath,
  rename,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import { basename, dirname, extname, isAbsolute, relative, resolve } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import type { FileMtime, FsEntry, GitPr, ProjectFile } from "../src/platform/tauri/fs";
import { hostWorktrees } from "./git-worktrees";
import { createHostBranch, hostBranches, switchHostBranch } from "./git-branches";
import type { HostStore } from "./store";
import {
  createHostPath,
  existingPath,
  hostFileDiff,
  hostGitAction,
  hostGitIndex,
  indexHostFiles,
  listHostFiles,
  searchHostContent,
  workspacePath,
} from "./workspace";

const exec = promisify(execFile);

/** Local file commands the host answers for a remote project, with the same
 * names, arguments and results as this app's Tauri commands, so the same UI
 * works on either machine. Paths are absolute host paths and must lie inside
 * a registered project or one of its worktrees. */
export const WORKSPACE_COMMANDS = [
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
] as const;
export type WorkspaceCommand = (typeof WORKSPACE_COMMANDS)[number];

// Remote RPC has bounded request and response bodies. Keep file operations
// within those bounds even after JSON escaping or base64 encoding.
const MAX_TEXT_FILE = 1024 * 1024;
const MAX_PREVIEW_FILE = 10 * 1024 * 1024;
const MAX_STAT_FILES = 64;
const ROOTS_TTL_MS = 5_000;

const slashed = (path: string) => path.replace(/\\/g, "/");
const joined = (parent: string, name: string) =>
  `${slashed(parent).replace(/\/+$/, "")}/${slashed(name).replace(/^\/+|\/+$/g, "")}`;
const alreadyExists = (name: string) =>
  `A file or folder ${name} already exists at this location. Please choose a different name.`;

type Located = { root: string; relative: string };

export type AzurePrTarget = {
  organizationUrl: string;
  project: string;
  repo: string;
};

const percentDecode = (value: string): string => {
  try {
    return decodeURIComponent(value).trim().replace(/\.git$/, "");
  } catch {
    return value.trim().replace(/\.git$/, "");
  }
};

const validRepoParts = (project: string, repo: string): boolean =>
  [project, repo].every(
    (part) =>
      part.length > 0 &&
      part !== "." &&
      part !== ".." &&
      !part.includes("/") &&
      !part.includes("?") &&
      !part.includes("#") &&
      !part.includes("\\") &&
      ![...part].some((char) => char < " "),
  );

/** Parses an Azure DevOps git remote into organization, project and repository.
 * Returns null for non-Azure remotes. Mirrors the Rust backend's
 * `parse_azure_remote` so local and remote projects resolve the same target. */
export function parseAzureDevOpsRemote(remote: string): AzurePrTarget | null {
  const trimmed = remote.trim();
  if (!trimmed) return null;
  // SSH without scheme: git@ssh.dev.azure.com:v3/{org}/{project}/{repo}
  // or on-premises scp-style user@host:{collection/...}/{project}/_git/{repo}.
  if (!trimmed.includes("://") && trimmed.includes("@")) {
    const colon = trimmed.indexOf(":");
    if (colon > 0) {
      let path = trimmed.slice(colon + 1);
      path = path.startsWith("v3/") ? path.slice(3) : path;
      const parts = path.split("/").filter(Boolean);
      if (parts.length >= 3 && trimmed.includes("dev.azure.com")) {
        const project = percentDecode(parts[1]!);
        const repo = percentDecode(parts.slice(2).join("/")).split("/").pop() ?? "";
        if (project && repo)
          return {
            organizationUrl: `https://dev.azure.com/${parts[0]!.toLowerCase()}`,
            project,
            repo,
          };
      }
      const hostBase = trimmed.slice(0, colon).split("@").pop()?.trim().toLowerCase() ?? "";
      if (hostBase) {
        // A leading numeric segment is the SSH port (host:port/path).
        let host = hostBase;
        let scpPath = path;
        const portSplit = scpPath.indexOf("/");
        const maybePort = portSplit < 0 ? scpPath : scpPath.slice(0, portSplit);
        if (/^\d+$/.test(maybePort)) {
          host = `${host}:${maybePort}`;
          scpPath = portSplit < 0 ? "" : scpPath.slice(portSplit + 1);
        }
        const segments = scpPath.split("/").filter(Boolean).map(percentDecode);
        const gitIndex = segments.findIndex((segment) => segment.toLowerCase() === "_git");
        if (gitIndex >= 2 && gitIndex + 1 < segments.length) {
          const project = segments[gitIndex - 1]!;
          const repo = segments.slice(gitIndex + 1).join("/");
          const orgPath = segments.slice(0, gitIndex - 1).join("/");
          if (orgPath && validRepoParts(project, repo))
            return {
              organizationUrl: `https://${host}/${orgPath}`.toLowerCase(),
              project,
              repo,
            };
        }
      }
    }
  }
  // HTTPS (credentials stripped first).
  const schemeSplit = trimmed.split("://");
  if (schemeSplit.length < 2) return null;
  const afterAuth = schemeSplit[1]!.split("@").pop() ?? "";
  const slash = afterAuth.indexOf("/");
  if (slash < 0) return null;
  const authority = afterAuth.slice(0, slash).toLowerCase();
  const segments = afterAuth
    .slice(slash + 1)
    .split("/")
    .filter(Boolean)
    .map(percentDecode);
  const lowerAuthority = authority.toLowerCase();
  if (lowerAuthority === "dev.azure.com") {
    // {org}/{project}/_git/{repo}
    if (segments.length >= 4 && segments[2]!.toLowerCase() === "_git") {
      const project = segments[1]!;
      const repo = segments.slice(3).join("/");
      if (validRepoParts(project, repo))
        return {
          organizationUrl: `https://dev.azure.com/${segments[0]!.toLowerCase()}`,
          project,
          repo,
        };
    }
    return null;
  }
  if (lowerAuthority.endsWith(".visualstudio.com")) {
    // {project}/_git/{repo}
    const org = lowerAuthority.slice(0, -".visualstudio.com".length);
    if (segments.length >= 3 && segments[1]!.toLowerCase() === "_git") {
      const project = segments[0]!;
      const repo = segments.slice(2).join("/");
      if (validRepoParts(project, repo))
        return { organizationUrl: `https://dev.azure.com/${org}`, project, repo };
    }
    return null;
  }
  // On-premises or custom host: {collection...}/{project}/_git/{repo}
  const gitIndex = segments.findIndex((segment) => segment.toLowerCase() === "_git");
  if (gitIndex < 2 || gitIndex + 1 >= segments.length) return null;
  const project = segments[gitIndex - 1]!;
  const repo = segments.slice(gitIndex + 1).join("/");
  const orgPath = segments.slice(0, gitIndex - 1).join("/");
  if (!validRepoParts(project, repo)) return null;
  return {
    organizationUrl: `https://${lowerAuthority}/${orgPath}`.toLowerCase(),
    project,
    repo,
  };
}

/** Canonical Azure Repos pull-request URL for a numeric PR id. Project and
 * repository names are encoded so spaces or reserved characters (e.g. a
 * literal `%2F` inside a name) cannot change the repository path. */
export function azurePrWebUrl(target: AzurePrTarget, id: number): string {
  return `${target.organizationUrl.replace(/\/+$/, "")}/${encodeURIComponent(target.project)}/_git/${encodeURIComponent(target.repo)}/pullrequest/${id}`;
}

/** Remote name in an `@{upstream}` abbrev ref (`origin/main` -> `origin`).
 * Returns null for local-only upstreams, where `git push` uses push.default. */
export function pushRemoteFromUpstream(upstream: string): string | null {
  const slash = upstream.trim().indexOf("/");
  if (slash <= 0) return null;
  return upstream.trim().slice(0, slash);
}

export class WorkspaceCommands {
  private roots = new Map<string, { at: number; roots: string[] }>();
  private rootsGeneration = 0;

  invalidateRoots(): void {
    this.rootsGeneration++;
    this.roots.clear();
  }

  constructor(
    private readonly store: HostStore,
    private readonly withIdleProject: <T>(projectId: string, action: () => Promise<T>) => Promise<T>,
  ) {}

  run(command: unknown, args: unknown): Promise<unknown> {
    if (!WORKSPACE_COMMANDS.includes(command as WorkspaceCommand))
      throw new Error("Unsupported workspace command");
    const input =
      args && typeof args === "object" && !Array.isArray(args)
        ? (args as Record<string, unknown>)
        : {};
    switch (command as WorkspaceCommand) {
      case "list_dir":
        return this.listDir(input.path);
      case "list_project_files":
        return this.listProjectFiles(input.cwd);
      case "read_text_file":
        return this.readText(input.path);
      case "read_binary_file":
        return this.readBinary(input.path);
      case "read_file_preview":
        return this.preview(input.path, input.maxLines, input.startLine);
      case "write_text_file":
        return this.writeText(input.path, input.content);
      case "stat_files":
        return this.statFiles(input.paths);
      case "create_path":
        return this.create(input.parent, input.name, input.isDir);
      case "rename_path":
        return this.rename(input.path, input.name);
      case "delete_path":
        return this.delete(input.path);
      case "copy_path":
        return this.copy(input.from, input.destParent);
      case "move_path":
        return this.move(input.from, input.destParent);
      case "git_diff_index":
      case "git_diff_files":
        return this.gitIndex(input.cwd);
      case "git_diff_stats":
        return this.gitIndex(input.cwd).then((index) => ({
          files: index.files.length,
          additions: index.additions,
          deletions: index.deletions,
        }));
      case "git_file_diff":
        return this.gitFileDiff(input.cwd, input.relative, input.staged);
      case "git_stage_contents":
        return this.gitAction(input.cwd, "stageContents", input.relative, undefined, input.contents);
      case "git_stage_file":
        return this.gitAction(input.cwd, "stage", input.relative);
      case "git_unstage_file":
        return this.gitAction(input.cwd, "unstage", input.relative);
      case "git_discard_file":
        return this.gitAction(input.cwd, "discard", input.relative);
      case "git_discard_all":
        return this.gitAction(input.cwd, "discardAll");
      case "git_stage_all":
        return this.gitAction(input.cwd, "stageAll");
      case "git_unstage_all":
        return this.gitAction(input.cwd, "unstageAll");
      case "git_commit":
        return this.gitCommit(input.cwd, input.message, input.amend);
      case "git_head_message":
        return this.gitCommand(input.cwd, ["log", "-1", "--format=%B"]);
      case "git_push":
        return this.gitAction(input.cwd, "push");
      case "git_pull":
        return this.gitCommand(input.cwd, ["pull", "--ff-only"]).then(() => undefined);
      case "git_sync":
        return this.gitSync(input.cwd);
      case "git_pr_status":
        return this.gitPrStatus(input.cwd);
      case "git_pr_create":
        return this.gitPrCreate(input.cwd, input.title, input.body, input.base, input.head);
      case "git_history":
        return this.gitHistory(input.cwd, input.limit);
      case "git_commit_files":
        return this.gitCommitFiles(input.cwd, input.sha);
      case "git_commit_file_diff":
        return this.gitCommitFileDiff(input.cwd, input.sha, input.relative);
      case "git_staged_context":
        return this.gitStagedContext(input.cwd);
      case "git_range_context":
        return this.gitRangeContext(input.cwd);
      case "git_branches":
        return this.gitBranches(input.cwd);
      case "git_checkout":
        return this.gitCheckout(input.cwd, input.name, input.remote);
      case "git_create_branch":
        return this.gitCreateBranch(input.cwd, input.name);
      case "git_stash":
        return this.gitStash(input.cwd, input.message);
      case "git_worktrees":
        return this.gitWorktrees(input.cwd);
      case "search_project":
        return this.searchProject(input.options);
    }
  }

  /** The project folders and worktrees files may be read and written in. */
  private async allowedRoots(): Promise<string[]> {
    const generation = this.rootsGeneration;
    const out: string[] = [];
    for (const project of this.store.projects()) {
      const cached = this.roots.get(project.cwd);
      if (cached && Date.now() - cached.at < ROOTS_TTL_MS) {
        out.push(...cached.roots);
        continue;
      }
      const roots = await hostWorktrees(project.cwd)
        .then((listed) => [
          project.cwd,
          ...listed.worktrees
            .filter((tree) => !tree.missing && tree.path !== project.cwd)
            .map((tree) => tree.path),
        ])
        .catch(() => [project.cwd]);
      if (generation === this.rootsGeneration)
        this.roots.set(project.cwd, { at: Date.now(), roots });
      out.push(...roots);
    }
    return out;
  }

  /** Finds the project root that contains `input`, which may not exist yet. */
  private async locate(input: unknown): Promise<Located> {
    if (
      typeof input !== "string" ||
      !isAbsolute(input) ||
      input.length > 4096 ||
      input.includes("\0")
    )
      throw new Error("Invalid workspace path");
    let actual = resolve(input);
    let missing = "";
    // Resolve symlinks on the nearest existing ancestor, then re-append the
    // part that does not exist yet (a file about to be created or written).
    for (;;) {
      const real = await realpath(actual).catch(() => undefined);
      if (real) {
        actual = missing ? resolve(real, missing) : real;
        break;
      }
      const parent = dirname(actual);
      if (parent === actual) break;
      missing = missing ? `${basename(actual)}/${missing}` : basename(actual);
      actual = parent;
    }
    for (const root of await this.allowedRoots()) {
      const rel = relative(root, actual);
      if (rel === "" || (!rel.startsWith("..") && !isAbsolute(rel)))
        return { root, relative: rel };
    }
    throw new Error("Path is outside this machine’s projects");
  }

  private async existing(input: unknown, allowRoot = false) {
    const { root, relative: rel } = await this.locate(input);
    return { root, path: await existingPath(root, rel, allowRoot) };
  }

  private async listDir(input: unknown): Promise<FsEntry[]> {
    const { root, relative: rel } = await this.locate(input);
    const entries = await listHostFiles(root, rel);
    return entries.map((entry) => ({
      ...entry,
      path: joined(input as string, entry.name),
    }));
  }

  private async listProjectFiles(input: unknown): Promise<ProjectFile[]> {
    const { path } = await this.existing(input, true);
    const cwd = input as string;
    return (await indexHostFiles(path)).map((file) => ({
      name: file.split("/").pop() || file,
      path: joined(cwd, file),
      relative: file,
    }));
  }

  private async file(input: unknown, limit: number, tooLarge: string) {
    const { path } = await this.existing(input);
    const info = await stat(path);
    if (!info.isFile()) throw new Error("Not a file");
    if (info.size > limit)
      throw new Error(
        `File is too large to ${tooLarge} (maximum ${limit / 1024 / 1024} MB).`,
      );
    return readFile(path);
  }

  private async readText(input: unknown): Promise<string> {
    const bytes = await this.file(input, MAX_TEXT_FILE, "edit");
    if (bytes.includes(0)) throw new Error("Binary files cannot be edited.");
    try {
      return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    } catch {
      throw new Error("File is not valid UTF-8.");
    }
  }

  /** Base64, since host responses are JSON; the client decodes it. */
  private async readBinary(input: unknown): Promise<string> {
    return (await this.file(input, MAX_PREVIEW_FILE, "preview")).toString(
      "base64",
    );
  }

  private async preview(
    input: unknown,
    maxLines: unknown,
    startLine: unknown,
  ): Promise<string[]> {
    const text = (await this.file(input, MAX_TEXT_FILE, "preview")).toString(
      "utf8",
    );
    if (text.includes("\0")) throw new Error("Binary file");
    const limit = Math.min(12, Math.max(1, Number(maxLines) || 6));
    const start = Math.max(1, Number(startLine) || 1);
    return text
      .split(/\r?\n/)
      .slice(start - 1, start - 1 + limit)
      .map((line) => (line.length > 200 ? `${line.slice(0, 199)}…` : line));
  }

  private async writeText(input: unknown, content: unknown): Promise<void> {
    if (typeof content !== "string")
      throw new Error("Invalid file content");
    if (Buffer.byteLength(content, "utf8") > MAX_TEXT_FILE)
      throw new Error(
        `File is too large to save (maximum ${MAX_TEXT_FILE / 1024 / 1024} MB).`,
      );
    const { root, relative: rel } = await this.locate(input);
    const path = workspacePath(root, rel);
    if (await stat(path).then((info) => info.isDirectory(), () => false))
      throw new Error("Cannot save text to a directory.");
    // Replace atomically, as the local command does.
    const temporary = `${path}.monocode-${process.pid}-${Date.now()}`;
    await writeFile(temporary, content, "utf8");
    await rename(temporary, path).catch(async (reason) => {
      await rm(temporary, { force: true });
      throw reason;
    });
  }

  private async statFiles(input: unknown): Promise<FileMtime[]> {
    if (!Array.isArray(input) || input.length > MAX_STAT_FILES)
      throw new Error("Too many paths");
    return Promise.all(
      input.map(async (path) => {
        const mtimeMs = await this.existing(path)
          .then(({ path: actual }) => stat(actual))
          .then((info) => (info.isFile() ? Math.floor(info.mtimeMs) : null))
          .catch(() => null);
        return { path: String(path), mtimeMs };
      }),
    );
  }

  private async create(
    parent: unknown,
    name: unknown,
    isDir: unknown,
  ): Promise<string> {
    const { root, relative: rel } = await this.locate(parent);
    try {
      await createHostPath(root, rel, name, isDir);
    } catch (reason) {
      if ((reason as NodeJS.ErrnoException).code === "EEXIST")
        throw new Error(alreadyExists(String(name)));
      throw reason;
    }
    return joined(parent as string, name as string);
  }

  private async rename(input: unknown, name: unknown): Promise<string> {
    if (
      typeof name !== "string" ||
      !name.trim() ||
      /^[\\/]/.test(name) ||
      name.includes("\0")
    )
      throw new Error("A file or folder name must be provided.");
    const { root, path: from } = await this.existing(input);
    const target = joined(dirname(input as string), name);
    const { root: targetRoot, relative: rel } = await this.locate(target);
    if (targetRoot !== root)
      throw new Error("Cannot move a file between working copies.");
    const to = workspacePath(root, rel);
    if (to === from) return input as string;
    if (await lstat(to).then(() => true, () => false))
      throw new Error(alreadyExists(name));
    if (!relative(from, to).startsWith(".."))
      throw new Error("Cannot move a folder into itself.");
    await mkdir(dirname(to), { recursive: true });
    await rename(from, to);
    return target;
  }

  private async delete(input: unknown): Promise<void> {
    const { path } = await this.existing(input);
    await rm(path, { recursive: true });
  }

  private async destination(from: unknown, destParent: unknown) {
    const source = await this.existing(from);
    const parent = await this.existing(destParent, true);
    if (!(await stat(parent.path)).isDirectory())
      throw new Error(`${String(destParent)} is not a folder`);
    if (
      (await stat(source.path)).isDirectory() &&
      !relative(source.path, parent.path).startsWith("..")
    )
      throw new Error("Cannot paste a folder into itself.");
    return { source: source.path, parent: parent.path };
  }

  private async copy(from: unknown, destParent: unknown): Promise<string> {
    const { source, parent } = await this.destination(from, destParent);
    const name = basename(source);
    const ext = extname(name);
    const stem = ext ? name.slice(0, -ext.length) : name;
    for (let n = 0; ; n++) {
      const candidate =
        n === 0 ? name : n === 1 ? `${stem} copy${ext}` : `${stem} copy ${n}${ext}`;
      const to = resolve(parent, candidate);
      if (await lstat(to).then(() => true, () => false)) continue;
      await cp(source, to, { recursive: true, errorOnExist: true });
      return joined(destParent as string, candidate);
    }
  }

  private async move(from: unknown, destParent: unknown): Promise<string> {
    const { source, parent } = await this.destination(from, destParent);
    const name = basename(source);
    const to = resolve(parent, name);
    if (to === source) return from as string;
    if (await lstat(to).then(() => true, () => false))
      throw new Error(alreadyExists(name));
    await rename(source, to);
    return joined(destParent as string, name);
  }

  private async gitRoot(input: unknown): Promise<string> {
    const { path } = await this.existing(input, true);
    if (!(await stat(path)).isDirectory()) throw new Error("Not a working copy");
    return path;
  }

  private async searchProject(input: unknown) {
    if (!input || typeof input !== "object" || Array.isArray(input))
      throw new Error("Invalid search");
    const options = input as Record<string, unknown>;
    return searchHostContent(await this.gitRoot(options.cwd), options);
  }

  private async gitIndex(input: unknown) {
    return hostGitIndex(await this.gitRoot(input));
  }

  private async gitFileDiff(cwd: unknown, relative: unknown, staged: unknown) {
    return hostFileDiff(await this.gitRoot(cwd), relative, staged === true);
  }

  private async gitAction(
    cwd: unknown,
    action: string,
    relative?: unknown,
    message?: unknown,
    contents?: unknown,
  ) {
    return hostGitAction(await this.gitRoot(cwd), action, relative, message, contents);
  }

  private async gitCommand(cwd: unknown, args: string[]): Promise<string> {
    const root = await this.gitRoot(cwd);
    return (await exec("git", ["-c", "core.pager=cat", ...args], {
      cwd: root,
      timeout: 30_000,
      maxBuffer: 4 * 1024 * 1024,
      encoding: "utf8",
      env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
    })).stdout;
  }

  private async gitCommit(cwd: unknown, message: unknown, amend: unknown) {
    if (typeof message !== "string" || !message.trim() || message.length > 100_000)
      throw new Error("Enter a commit message");
    await this.gitCommand(cwd, ["commit", ...(amend === true ? ["--amend"] : []), "-m", message]);
  }

  private async gitSync(cwd: unknown) {
    await this.gitCommand(cwd, ["pull", "--ff-only"]);
    await this.gitAction(cwd, "push");
  }

  private async ghCommand(cwd: unknown, args: string[]): Promise<string> {
    return this.cliCommand(cwd, "gh", args, {
      GH_PROMPT_DISABLED: "1",
      GIT_TERMINAL_PROMPT: "0",
    });
  }

  private async cliCommand(
    cwd: unknown,
    bin: string,
    args: string[],
    env: Record<string, string>,
  ): Promise<string> {
    const root = await this.gitRoot(cwd);
    return (await exec(bin, args, {
      cwd: root,
      timeout: 30_000,
      maxBuffer: 1024 * 1024,
      encoding: "utf8",
      env: { ...process.env, ...env, GIT_TERMINAL_PROMPT: "0" },
    })).stdout.trim();
  }

  /** Remote `git push` would use: the upstream's remote, else origin if
   * present, else the first remote. Mirrors the desktop backend so PR
   * creation targets the same repository the branch pushes to. */
  private async gitPushRemote(root: string): Promise<string | null> {
    const upstream = await this.gitCommand(root, ["rev-parse", "--abbrev-ref", "@{upstream}"])
      .then((output) => output.trim())
      .catch(() => "");
    if (upstream) return pushRemoteFromUpstream(upstream);
    const remotes = await this.gitCommand(root, ["remote"])
      .then((output) => output.split("\n").map((name) => name.trim()).filter(Boolean))
      .catch(() => [] as string[]);
    if (remotes.length === 0) return null;
    return remotes.includes("origin") ? "origin" : remotes[0]!;
  }

  /** Azure DevOps coordinates of the branch's push destination, or null when
   * the push remote is not an Azure DevOps remote. Resolves the push URL
   * (not the fetch URL) so `remote.<name>.pushurl` configurations target
   * the repository that actually receives the push. */
  private async azurePrTarget(root: string): Promise<AzurePrTarget | null> {
    const remote = await this.gitPushRemote(root);
    if (!remote) return null;
    const url = await this.gitCommand(root, ["remote", "get-url", "--push", remote])
      .then((output) => output.trim())
      .catch(() => "");
    if (!url) return null;
    return parseAzureDevOpsRemote(url);
  }

  /** Creates the PR with the Azure CLI and returns its web URL. Throws when
   * `az` is unavailable or creation fails, so the caller can fall back to `gh`. */
  private async azurePrCreate(
    root: string,
    target: AzurePrTarget,
    title: string,
    body: string,
    base: string,
    head: string,
  ): Promise<string> {
    const output = await exec("az", [
      "repos", "pr", "create",
      "--organization", target.organizationUrl,
      "--project", target.project,
      "--repository", target.repo,
      "--source-branch", head,
      "--target-branch", base,
      "--title", title,
      "--description", body,
      "--output", "json",
    ], {
      cwd: root,
      timeout: 30_000,
      maxBuffer: 1024 * 1024,
      encoding: "utf8",
      env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
    }).then((result) => result.stdout.trim());
    const id = Number((JSON.parse(output) as { pullRequestId?: unknown }).pullRequestId);
    if (!Number.isInteger(id) || id <= 0) throw new Error("Azure DevOps did not return a pull request");
    return azurePrWebUrl(target, id);
  }

  private async gitPrStatus(cwd: unknown) {
    const output = await this.ghCommand(cwd, ["pr", "view", "--json", "number,title,url,state"])
      .catch(() => "");
    if (!output) return null;
    const pr = JSON.parse(output) as GitPr;
    return { ...pr, state: pr.state.toLowerCase() };
  }

  private async gitPrCreate(cwd: unknown, title: unknown, body: unknown, base: unknown, head: unknown) {
    if ([title, body, base, head].some((value) => typeof value !== "string" || value.length > 100_000))
      throw new Error("Invalid pull request");
    const strings = [title, body, base, head] as string[];
    if (!strings[0]!.trim() || !strings[2]!.trim() || !strings[3]!.trim())
      throw new Error("Invalid pull request");
    // Azure Repos checkouts use the Azure CLI; anything else (or an `az`
    // failure) falls back to the GitHub CLI path.
    const root = await this.gitRoot(cwd).catch(() => null);
    if (root) {
      const target = await this.azurePrTarget(root).catch(() => null);
      if (target) {
        try {
          return await this.azurePrCreate(root, target, strings[0]!, strings[1]!, strings[2]!, strings[3]!);
        } catch {
          // Fall through to `gh`, matching the desktop backend fallback.
        }
      }
    }
    return this.ghCommand(cwd, ["pr", "create", "--title", title as string, "--body", body as string, "--base", base as string, "--head", head as string]);
  }

  private gitSha(value: unknown): string {
    if (typeof value !== "string" || !/^[0-9a-f]{4,40}$/i.test(value))
      throw new Error("Invalid commit");
    return value;
  }

  private async gitCommitSha(cwd: unknown, input: unknown): Promise<string> {
    const sha = this.gitSha(input);
    const resolved = await this.gitCommand(cwd, ["rev-parse", "--verify", `${sha}^{commit}`])
      .catch(() => "");
    if (!/^[0-9a-f]{40,64}$/i.test(resolved.trim()))
      throw new Error("Unknown commit");
    return resolved.trim();
  }

  private async gitHistory(cwd: unknown, limit: unknown) {
    const count = Number.isSafeInteger(limit) ? Math.min(500, Math.max(1, Number(limit))) : 200;
    const [head, upstream, index, remoteNames] = await Promise.all([
      this.gitCommand(cwd, ["rev-parse", "--verify", "HEAD"]).catch(() => ""),
      this.gitCommand(cwd, ["rev-parse", "--abbrev-ref", "@{upstream}"]).catch(() => ""),
      this.gitIndex(cwd),
      this.gitCommand(cwd, ["remote"]).catch(() => ""),
    ]);
    const headSha = head.trim() || null;
    if (!headSha) return { head: null, commits: [] };
    const tips = ["HEAD"];
    if (upstream.trim()) tips.push("@{upstream}");
    if (index.defaultBranch && index.remote) {
      const defaultRef = `refs/remotes/origin/${index.defaultBranch}`;
      const exists = await this.gitCommand(cwd, ["rev-parse", "--verify", defaultRef])
        .then(() => true, () => false);
      if (exists) tips.push(`origin/${index.defaultBranch}`);
    }
    const output = await this.gitCommand(cwd, [
      "log", "--topo-order", "--decorate=short", `--max-count=${count}`,
      "--format=%H%x00%h%x00%P%x00%an%x00%at%x00%D%x00%s%x1e", ...tips,
    ]).catch(() => "");
    const remotes = remoteNames.split("\n").map((name) => name.trim()).filter(Boolean);
    const commits = output.split("\x1e").flatMap((record) => {
      const [sha, shortSha, parents, author, timestamp, decorations, subject] = record.trim().split("\0");
      if (!sha || !/^[0-9a-f]{40,64}$/i.test(sha)) return [];
      const refs = (decorations ?? "").split(",").map((raw) => raw.trim()).filter(Boolean)
        .flatMap((raw) => {
          if (raw === "HEAD" || raw.endsWith("/HEAD")) return [];
          if (raw.startsWith("HEAD -> ")) return [{ name: raw.slice(8), kind: "local" }];
          if (raw.startsWith("tag: ")) return [{ name: raw.slice(5), kind: "tag" }];
          return [{ name: raw, kind: remotes.some((remote) => raw === remote || raw.startsWith(`${remote}/`))
            ? "remote" : "local" }];
        });
      return [{ sha, shortSha: shortSha || sha.slice(0, 7), parents: parents ? parents.split(" ") : [],
        author, timestamp: Number(timestamp), subject, refs, head: sha === headSha }];
    });
    return { head: headSha, commits };
  }

  private async gitCommitFiles(cwd: unknown, shaInput: unknown) {
    const sha = await this.gitCommitSha(cwd, shaInput);
    const [names, stats] = await Promise.all([
      this.gitCommand(cwd, ["diff-tree", "--root", "--no-commit-id", "--no-renames", "--name-status", "-r", sha]),
      this.gitCommand(cwd, ["diff-tree", "--root", "--no-commit-id", "--no-renames", "--numstat", "-r", sha]),
    ]);
    const counts = new Map(stats.split("\n").filter(Boolean).map((line) => {
      const [added, removed, path] = line.split("\t");
      return [path, { additions: Number(added) || 0, deletions: Number(removed) || 0 }] as const;
    }));
    return names.split("\n").filter(Boolean).map((line) => {
      const [code, relativePath] = line.split("\t");
      return { path: relativePath, relative: relativePath,
        status: code === "A" ? "added" : code === "D" ? "deleted" : "modified",
        ...(counts.get(relativePath) ?? { additions: 0, deletions: 0 }),
        staged: false, unstaged: false };
    });
  }

  private async gitCommitFileDiff(cwd: unknown, shaInput: unknown, input: unknown) {
    const sha = await this.gitCommitSha(cwd, shaInput);
    const root = await this.gitRoot(cwd);
    const path = relative(root, workspacePath(root, input)).replace(/\\/g, "/");
    const parent = await this.gitCommand(root, ["rev-parse", "--verify", `${sha}^`]).catch(() => "");
    const [original, current] = await Promise.all([
      parent ? this.gitBlob(root, `${parent.trim()}:${path}`) : Promise.resolve({ bytes: Buffer.alloc(0), tooLarge: false }),
      this.gitBlob(root, `${sha}:${path}`),
    ]);
    const binary = original.bytes.includes(0) || current.bytes.includes(0);
    const tooLarge = original.tooLarge || current.tooLarge ||
      original.bytes.length > MAX_TEXT_FILE || current.bytes.length > MAX_TEXT_FILE;
    const status = !original.bytes.length && current.bytes.length ? "added"
      : original.bytes.length && !current.bytes.length ? "deleted" : "modified";
    return { path, relative: path, status,
      original: binary || tooLarge ? "" : original.bytes.toString("utf8"),
      current: binary || tooLarge ? "" : current.bytes.toString("utf8"),
      binary, tooLarge };
  }

  private async gitBlob(root: string, spec: string) {
    try {
      const { stdout } = await exec("git", ["show", spec], {
        cwd: root,
        timeout: 30_000,
        maxBuffer: MAX_TEXT_FILE + 1024,
        encoding: "buffer",
        env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
      });
      return { bytes: Buffer.isBuffer(stdout) ? stdout : Buffer.from(stdout), tooLarge: false };
    } catch (reason) {
      return { bytes: Buffer.alloc(0), tooLarge: String(reason).includes("maxBuffer") };
    }
  }

  private async gitStagedContext(cwd: unknown) {
    const [index, summary, patch] = await Promise.all([
      this.gitIndex(cwd),
      this.gitCommand(cwd, ["diff", "--cached", "--stat"]),
      this.gitCommand(cwd, ["diff", "--cached", "--no-ext-diff"]),
    ]);
    return { branch: index.branch, summary, patch };
  }

  private async gitRangeContext(cwd: unknown) {
    const index = await this.gitIndex(cwd);
    if (!index.defaultBranch) throw new Error("Default branch is unknown");
    const base = `origin/${index.defaultBranch}`;
    const head = index.branch ?? "HEAD";
    const [commitSummary, diffSummary, diffPatch] = await Promise.all([
      this.gitCommand(cwd, ["log", "--oneline", `${base}..HEAD`]),
      this.gitCommand(cwd, ["diff", "--stat", `${base}...HEAD`]),
      this.gitCommand(cwd, ["diff", "--no-ext-diff", `${base}...HEAD`]),
    ]);
    return { base: index.defaultBranch, head, commitSummary, diffSummary, diffPatch };
  }

  private async gitBranches(cwd: unknown) {
    const state = await hostBranches(await this.gitRoot(cwd));
    return { current: state.current, detached: state.current === null,
      branches: [
        ...state.branches.map((name) => ({ name, current: name === state.current, remote: null })),
        ...state.remotes.map((entry) => ({ name: entry.name, current: false, remote: entry.remote })),
      ] };
  }

  private async gitCheckout(cwd: unknown, name: unknown, remote: unknown) {
    const root = await this.gitRoot(cwd);
    const state = await this.withIdleGitProject(root, () => switchHostBranch(root, name, remote));
    return state.current ?? "HEAD";
  }

  private async gitCreateBranch(cwd: unknown, name: unknown) {
    const root = await this.gitRoot(cwd);
    const state = await this.withIdleGitProject(root, () => createHostBranch(root, name));
    return state.current ?? "HEAD";
  }

  private async withIdleGitProject<T>(cwd: string, action: () => Promise<T>): Promise<T> {
    const { root } = await this.locate(cwd);
    const project = this.store.projects().find((candidate) =>
      candidate.cwd === root || this.roots.get(candidate.cwd)?.roots.includes(root));
    if (!project) throw new Error("Project is unavailable");
    return this.withIdleProject(project.id, action);
  }

  private async gitStash(cwd: unknown, message: unknown) {
    if (message != null && (typeof message !== "string" || message.length > 1000))
      throw new Error("Invalid stash message");
    await this.gitCommand(cwd, ["stash", "push", "-u", ...(message ? ["-m", message] : [])]);
  }

  private async gitWorktrees(cwd: unknown) {
    const listed = await hostWorktrees(await this.gitRoot(cwd));
    return {
      defaultRoot: listed.defaultRoot,
      worktrees: listed.worktrees.map((tree) => ({
        ...tree,
        locked: false,
        prunable: tree.missing,
        dirty: null,
        unpushed: null,
        sessionIds: [],
      })),
    };
  }
}

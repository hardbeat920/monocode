import type { ExecFileOptions } from "node:child_process";
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import { hostBranches } from "./git-branches";
import { hostWorktrees, resolveHostWorktree } from "./git-worktrees";
import { hostGitAction, hostGitIndex } from "./workspace";
import { WorkspaceCommands } from "./workspace-commands";
import { HostStore } from "./store";

const launches = vi.hoisted(() => [] as Array<{
  kind: string;
  file: string;
  options: { windowsHide?: boolean };
}>);

// Run real Git operations while observing the options passed to Node. A test
// launched from a terminal already has a console, which can mask this bug.
vi.mock("node:child_process", async (importOriginal) => {
  const actual = await importOriginal<typeof import("node:child_process")>();
  const { promisify } = await import("node:util");
  const run = promisify(actual.execFile);
  return {
    ...actual,
    execFile: Object.assign(actual.execFile.bind(null), {
      [promisify.custom]: (file: string, args: string[], options: ExecFileOptions) => {
        launches.push({ kind: "execFile", file, options });
        // Keep the test independent of GitHub authentication and network access.
        if (file === "gh") return Promise.resolve({ stdout: "", stderr: "" });
        return run(file, args, options);
      },
    }),
    execFileSync: (...args: Parameters<typeof actual.execFileSync>) => {
      launches.push({ kind: "execFileSync", file: args[0], options: args[2] ?? {} });
      return actual.execFileSync(...args);
    },
    spawn: (...args: Parameters<typeof actual.spawn>) => {
      launches.push({ kind: "spawn", file: args[0], options: args[2] ?? {} });
      return actual.spawn(...args);
    },
  };
});

const cleanups: Array<() => void> = [];
afterEach(() => {
  for (const cleanup of cleanups.splice(0).reverse()) cleanup();
  launches.length = 0;
});

it("hides consoles for Git polling, worktree validation, staging and GitHub commands", async () => {
  // Bypass the observer for fixture setup; only production launches count.
  const { execFileSync } = await vi.importActual<typeof import("node:child_process")>("node:child_process");
  const directory = realpathSync.native(mkdtempSync(join(tmpdir(), "monocode-background-git-")));
  cleanups.push(() => rmSync(directory, { recursive: true, force: true }));
  const cwd = join(directory, "repo");
  mkdirSync(cwd);
  const git = (...args: string[]) => execFileSync("git", args, {
    cwd, windowsHide: true, encoding: "utf8",
  }).trim();
  git("init", "-q", "-b", "main");
  git("config", "user.name", "Test");
  git("config", "user.email", "test@example.com");
  git("config", "commit.gpgsign", "false");
  git("config", "core.autocrlf", "false");
  writeFileSync(join(cwd, "file.txt"), "before\n");
  git("add", "file.txt");
  git("commit", "-qm", "Initial");
  const worktree = join(directory, "worktree");
  git("worktree", "add", "-qb", "feature", worktree);

  expect((await hostBranches(cwd)).current).toBe("main");
  expect((await hostWorktrees(cwd)).worktrees).toHaveLength(2);
  expect(resolveHostWorktree(cwd, worktree)).toBe(worktree);
  await hostGitAction(cwd, "stageContents", "file.txt", undefined, "after\n");
  expect(git("show", ":file.txt")).toBe("after");
  expect((await hostGitIndex(cwd)).files).toHaveLength(1);
  await hostGitAction(cwd, "createPr");

  const store = new HostStore(join(directory, "host.db"));
  cleanups.push(() => store.close());
  store.addProject(cwd, "Test");
  const commands = new WorkspaceCommands(store, async (_id, action) => action());
  await commands.run("git_locate_files", { paths: [join(cwd, "file.txt")] });
  expect(String(await commands.run("git_head_message", { cwd })).trim()).toBe("Initial");
  await commands.run("git_pr_status", { cwd });
  await commands.run("git_commit_file_diff", {
    cwd, sha: git("rev-parse", "HEAD"), relative: "file.txt",
  });

  expect(new Set(launches.map(({ kind }) => kind))).toEqual(
    new Set(["execFile", "execFileSync", "spawn"]),
  );
  expect(launches.some(({ file }) => file === "gh")).toBe(true);
  for (const launch of launches)
    expect(launch.options.windowsHide, `${launch.kind}: ${launch.file}`).toBe(true);
});

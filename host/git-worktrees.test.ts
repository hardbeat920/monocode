import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import { HostEngine } from "./engine";
import { HostStore } from "./store";
import {
  createHostWorktree,
  hostWorktrees,
  renameHostWorktreeBranch,
  resolveHostWorktree,
} from "./git-worktrees";

const cleanups: Array<() => Promise<void> | void> = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0).reverse()) await cleanup();
});

it("generates a remote session title and renames its new worktree branch", async () => {
  const cwd = mkdtempSync(join(tmpdir(), "monocode-host-names-"));
  const git = (...args: string[]) => execFileSync("git", args, { cwd });
  git("init", "-q");
  git("checkout", "-q", "-b", "main");
  writeFileSync(join(cwd, "file.txt"), "initial\n");
  git("add", "file.txt");
  git(
    "-c",
    "user.name=Test",
    "-c",
    "user.email=test@example.com",
    "commit",
    "-q",
    "-m",
    "initial",
  );
  const store = new HostStore(join(cwd, "host.db"));
  const title = vi.fn(async () => ({
    title: "Fix remote naming",
    workItem: null,
  }));
  const branch = vi.fn(async () => "remote-naming");
  const engine = new HostEngine(store, {
    codex: {
      send: async () => {},
      cancel: async () => {},
      stop: async () => {},
      bind: () => {},
      approve: () => {},
      answer: () => {},
      generateTitle: title,
      generateBranchName: branch,
    },
  });
  const project = store.addProject(cwd, "Test");
  const root = (await hostWorktrees(cwd)).defaultRoot;
  cleanups.push(async () => {
    await engine.close();
    store.close();
    rmSync(root, { recursive: true, force: true });
    rmSync(cwd, { recursive: true, force: true });
  });
  const tree = await createHostWorktree(cwd, "mc/12345678", "HEAD", false);
  const { sessionId } = engine.command({
    type: "create",
    commandId: "create-named",
    projectId: project.id,
    worktreeCwd: tree.path,
    autoWorktreeBranch: tree.branch!,
    harness: "codex",
    model: "codex:test",
    runtimeMode: "supervised",
  });
  engine.command({
    type: "send",
    commandId: "first",
    sessionId,
    text: "Fix remote session and worktree naming",
  });
  await vi.waitFor(() => {
    expect(store.session(sessionId).session).toMatchObject({
      title: "codex · Fix remote naming",
      branch: "mc/remote-naming",
      worktreeCwd: tree.path,
    });
  });
  expect(
    (await hostWorktrees(cwd)).worktrees.find((item) => item.path === tree.path)
      ?.branch,
  ).toBe("mc/remote-naming");
  expect(title).toHaveBeenCalledTimes(1);
  expect(branch).toHaveBeenCalledTimes(1);
  engine.command({
    type: "send",
    commandId: "second",
    sessionId,
    text: "More work",
  });
  await vi.waitFor(() => expect(store.session(sessionId).status).toBe("idle"));
  expect(title).toHaveBeenCalledTimes(1);
  expect(branch).toHaveBeenCalledTimes(1);
  await expect(
    renameHostWorktreeBranch(cwd, tree.path, "mc/12345678", "mc/other"),
  ).rejects.toThrow("changed");
});

it("creates registered host worktrees and binds new sessions to the selected checkout", async () => {
  const cwd = mkdtempSync(join(tmpdir(), "monocode-host-worktrees-"));
  const git = (...args: string[]) => execFileSync("git", args, { cwd });
  git("init", "-q");
  git("checkout", "-q", "-b", "main");
  writeFileSync(join(cwd, "file.txt"), "initial\n");
  git("add", "file.txt");
  git(
    "-c",
    "user.name=Test",
    "-c",
    "user.email=test@example.com",
    "commit",
    "-q",
    "-m",
    "initial",
  );
  git("branch", "existing");
  const store = new HostStore(join(cwd, "host.db"));
  const engine = new HostEngine(store, {
    codex: {
      send: async () => {},
      cancel: async () => {},
      stop: async () => {},
      bind: () => {},
      approve: () => {},
      answer: () => {},
    },
  });
  const project = store.addProject(cwd, "Test");
  const root = (await hostWorktrees(cwd)).defaultRoot;
  cleanups.push(async () => {
    await engine.close();
    store.close();
    rmSync(root, { recursive: true, force: true });
    rmSync(cwd, { recursive: true, force: true });
  });

  const tree = await createHostWorktree(cwd, "feature/task", "HEAD", false);
  expect(tree).toMatchObject({
    branch: "feature/task",
    isMain: false,
    missing: false,
  });
  expect(resolveHostWorktree(cwd, tree.path)).toBe(tree.path);
  const listed = await hostWorktrees(cwd);
  expect(listed.worktrees.map((item) => item.branch)).toEqual([
    "main",
    "feature/task",
  ]);
  const receipt = engine.command({
    type: "create",
    commandId: "worktree-session",
    projectId: project.id,
    worktreeCwd: tree.path,
    harness: "codex",
    model: "codex:test",
    runtimeMode: "supervised",
  });
  expect(store.session(receipt.sessionId).session.cwd).toBe(tree.path);
  expect(() =>
    engine.command({
      type: "create",
      commandId: "outside",
      projectId: project.id,
      worktreeCwd: tmpdir(),
      harness: "codex",
      model: "codex:test",
      runtimeMode: "supervised",
    }),
  ).toThrow("Choose an available worktree");
  await expect(
    createHostWorktree(cwd, "feature/task", "HEAD", false),
  ).rejects.toThrow("already has a working copy");
  const existing = await createHostWorktree(cwd, "existing", "HEAD", true);
  expect(existing.branch).toBe("existing");
});

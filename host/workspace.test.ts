import { afterEach, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  createHostPath,
  hostFileDiff,
  hostGitAction,
  hostGitIndex,
  hostGitStats,
  invalidateGitIndexCache,
  listHostFiles,
  readHostFile,
  searchHostContent,
  searchHostFiles,
  writeHostFile,
} from "./workspace";
import { getGitExecStats, resetGitExecStats } from "./git-exec";

const roots: string[] = [];
afterEach(() => {
  for (const root of roots.splice(0))
    rmSync(root, { recursive: true, force: true });
});

it("reports a broken Git index instead of searching ignored files", async () => {
  const root = mkdtempSync(join(tmpdir(), "monocode-broken-index-"));
  roots.push(root);
  execFileSync("git", ["init", "-q"], { cwd: root });
  writeFileSync(join(root, ".gitignore"), "private.txt\n");
  writeFileSync(join(root, "private.txt"), "ignored");
  writeFileSync(join(root, ".git", "index"), "broken");
  await expect(searchHostFiles(root, "private")).rejects.toThrow();
});

it("lists host files and rejects paths escaping the project", async () => {
  const root = realpathSync.native(mkdtempSync(join(tmpdir(), "monocode-workspace-")));
  roots.push(root);
  mkdirSync(join(root, "src"));
  writeFileSync(join(root, "src", "app.ts"), "source\n");
  symlinkSync(
    tmpdir(),
    join(root, "outside"),
    process.platform === "win32" ? "junction" : "dir",
  );
  expect((await listHostFiles(root, "")).map((entry) => entry.name)).toContain(
    "src",
  );
  expect(
    (await searchHostFiles(root, "app")).map((entry) => entry.path),
  ).toEqual(["src/app.ts"]);
  expect(await readHostFile(root, "src/app.ts")).toBe("source\n");
  expect(await readHostFile(root, join(root, "src", "app.ts"))).toBe(
    "source\n",
  );
  await writeHostFile(root, "src/app.ts", "source\n", "edited\n");
  expect(await readHostFile(root, "src/app.ts")).toBe("edited\n");
  expect(await searchHostContent(root, { query: "edited" })).toMatchObject({
    matches: [
      expect.objectContaining({ relative: "src/app.ts", line: 1, column: 1 }),
    ],
    truncated: false,
  });
  expect(await createHostPath(root, "src", "nested/new.ts", false)).toBe(
    "src/nested/new.ts",
  );
  expect(await readHostFile(root, "src/nested/new.ts")).toBe("");
  expect(await createHostPath(root, "", "assets", true)).toBe("assets");
  await expect(createHostPath(root, "src", "app.ts", false)).rejects.toThrow();
  await expect(
    createHostPath(root, "", "../outside.txt", false),
  ).rejects.toThrow("Invalid file name");
  await expect(createHostPath(root, "", ".git/config", false)).rejects.toThrow(
    "Invalid file name",
  );
  await expect(
    createHostPath(root, "outside", "bad.ts", false),
  ).rejects.toThrow("outside");
  await expect(
    writeHostFile(root, "src/app.ts", "source\n", "lost\n"),
  ).rejects.toThrow("changed on the host");
  await expect(listHostFiles(root, "../")).rejects.toThrow("outside");
  await expect(listHostFiles(root, "outside")).rejects.toThrow("outside");
  await expect(readHostFile(root, ".git/config")).rejects.toThrow("outside");
  expect(await hostGitIndex(root)).toMatchObject({
    branch: null,
    files: [],
    additions: 0,
    deletions: 0,
  });
});

it("reports tracked and untracked changes and commits staged files", async () => {
  const root = realpathSync.native(
    mkdtempSync(join(tmpdir(), "monocode-workspace-git-")),
  );
  roots.push(root);
  const git = (...args: string[]) => execFileSync("git", args, { cwd: root });
  git("init", "-q");
  git("config", "core.autocrlf", "false");
  git("config", "user.name", "Workspace Test");
  git("config", "user.email", "workspace@example.test");
  writeFileSync(join(root, "app.ts"), "before\n");
  git("add", "--", "app.ts");
  git("commit", "-qm", "initial");
  writeFileSync(join(root, "app.ts"), "after\n");
  writeFileSync(join(root, "new.ts"), "new\n");

  const index = await hostGitIndex(root);
  expect(
    await searchHostContent(root, {
      query: "AFTER",
      include: "app.ts",
      caseSensitive: false,
    }),
  ).toMatchObject({
    matches: [
      expect.objectContaining({ relative: "app.ts", line: 1, column: 1 }),
    ],
  });
  expect(
    (
      await searchHostContent(root, {
        query: "after",
        exclude: "app.ts",
      })
    ).matches,
  ).toEqual([]);
  expect(
    (
      await searchHostContent(root, {
        query: "AFTER",
        caseSensitive: true,
      })
    ).matches,
  ).toEqual([]);
  expect(
    (
      await searchHostContent(root, {
        query: "aft",
        wholeWord: true,
      })
    ).matches,
  ).toEqual([]);
  expect(
    (
      await searchHostContent(root, {
        query: "aft.r",
        regex: true,
      })
    ).matches,
  ).toEqual([expect.objectContaining({ relative: "app.ts", line: 1 })]);
  expect(index.files).toEqual(
    expect.arrayContaining([
      expect.objectContaining({
        relative: "app.ts",
        status: "modified",
        unstaged: true,
      }),
      expect.objectContaining({
        relative: "new.ts",
        status: "untracked",
        unstaged: true,
      }),
    ]),
  );
  expect(await hostFileDiff(root, "app.ts", false)).toMatchObject({
    original: "before\n",
    current: "after\n",
  });
  await expect(hostGitAction(root, "stage", "../outside")).rejects.toThrow(
    "outside",
  );
  await hostGitAction(root, "stageAll");
  expect((await hostGitIndex(root)).files.every((file) => file.staged)).toBe(
    true,
  );
  expect(await hostFileDiff(root, "app.ts", true)).toMatchObject({
    original: "before\n",
    current: "after\n",
  });
  await hostGitAction(root, "commit", undefined, "remote commit");
  expect((await hostGitIndex(root)).files).toEqual([]);
  const remote = realpathSync.native(
    mkdtempSync(join(tmpdir(), "monocode-workspace-remote-")),
  );
  roots.push(remote);
  execFileSync("git", ["init", "--bare", "-q"], { cwd: remote });
  git("remote", "add", "origin", remote);
  git("push", "-u", "origin", "HEAD");
  writeFileSync(join(root, "app.ts"), "another change\n");
  git("add", "app.ts");
  git("commit", "-qm", "ahead");
  expect(await hostGitIndex(root)).toMatchObject({
    remote: "origin",
    ahead: 1,
    behind: 0,
  });
  writeFileSync(join(root, "app.ts"), "discard this\n");
  await hostGitAction(root, "discard", "app.ts");
  expect((await hostGitIndex(root)).files).toEqual([]);
});

it("stages and unstages a folder subtree without affecting other changes", async () => {
  const root = realpathSync.native(
    mkdtempSync(join(tmpdir(), "monocode-host-folder-")),
  );
  roots.push(root);
  const git = (...args: string[]) => execFileSync("git", args, { cwd: root });
  git("init", "-q");
  git("config", "core.autocrlf", "false");
  git("config", "user.name", "Workspace Test");
  git("config", "user.email", "workspace@example.test");
  git("config", "commit.gpgsign", "false");
  mkdirSync(join(root, "src/nested"), { recursive: true });
  mkdirSync(join(root, "src-other"));
  writeFileSync(join(root, "src/app.ts"), "before\n");
  writeFileSync(join(root, "src/nested/deleted.ts"), "delete me\n");
  writeFileSync(join(root, "src-other/app.ts"), "before\n");
  writeFileSync(join(root, "ready.txt"), "before\n");
  writeFileSync(join(root, ".gitignore"), "src/ignored.txt\n");
  git("add", ".");
  git("commit", "-qm", "initial");
  writeFileSync(join(root, "src/app.ts"), "after\n");
  rmSync(join(root, "src/nested"), { recursive: true });
  mkdirSync(join(root, "src/added"));
  writeFileSync(join(root, "src/added/new.ts"), "new\n");
  writeFileSync(join(root, "src/ignored.txt"), "ignored\n");
  writeFileSync(join(root, "src-other/app.ts"), "outside\n");
  writeFileSync(join(root, "ready.txt"), "ready\n");
  await hostGitAction(root, "stage", "ready.txt");

  await hostGitAction(root, "stage", "src");
  let files = (await hostGitIndex(root)).files;
  expect(files).toHaveLength(5);
  for (const file of files) {
    expect(file.staged).toBe(
      file.relative.startsWith("src/") || file.relative === "ready.txt",
    );
    expect(file.unstaged).toBe(file.relative === "src-other/app.ts");
  }

  await hostGitAction(root, "unstage", "src");
  files = (await hostGitIndex(root)).files;
  expect(files).toHaveLength(5);
  for (const file of files) {
    expect(file.staged).toBe(file.relative === "ready.txt");
    expect(file.unstaged).toBe(file.relative !== "ready.txt");
  }
  expect(await readHostFile(root, "src/app.ts")).toBe("after\n");

  // A folder can still appear in the Changes tree after it was deleted on disk.
  await hostGitAction(root, "stage", "src/nested");
  expect(
    (await hostGitIndex(root)).files.find(
      (file) => file.relative === "src/nested/deleted.ts",
    ),
  ).toMatchObject({ staged: true, unstaged: false });
  await hostGitAction(root, "unstage", "src/nested");
  expect(
    (await hostGitIndex(root)).files.find(
      (file) => file.relative === "src/nested/deleted.ts",
    ),
  ).toMatchObject({ staged: false, unstaged: true });
});

it
  .runIf(process.platform !== "win32")
  .each(["*", "folder?", "[ab]", ":(glob)*"])(
  "stages and unstages the literal folder %s without touching siblings",
  async (folder) => {
    const root = realpathSync.native(
      mkdtempSync(join(tmpdir(), "monocode-host-literal-folder-")),
    );
    roots.push(root);
    const git = (...args: string[]) =>
      execFileSync("git", args, { cwd: root, encoding: "utf8" });
    git("init", "-q");
    git("config", "core.autocrlf", "false");
    git("config", "user.name", "Workspace Test");
    git("config", "user.email", "workspace@example.test");
    git("config", "commit.gpgsign", "false");
    const inside = `${folder}/inside.txt`;
    for (const directory of [folder, "a", "folderx"])
      mkdirSync(join(root, directory));
    const tracked = [inside, "a/other.txt", "folderx/other.txt", "ready.txt"];
    for (const path of tracked) writeFileSync(join(root, path), "before\n");
    git("add", ".");
    git("commit", "-qm", "initial");
    for (const path of tracked) writeFileSync(join(root, path), "after\n");
    writeFileSync(join(root, "private.txt"), "unrelated untracked data\n");
    const stagedPaths = () =>
      git("diff", "--cached", "--name-only", "-z").split("\0").filter(Boolean);

    await hostGitAction(root, "stage", "ready.txt");
    await hostGitAction(root, "stage", folder);
    expect(stagedPaths()).toEqual([inside, "ready.txt"].sort());

    await hostGitAction(root, "unstage", folder);
    expect(stagedPaths()).toEqual(["ready.txt"]);
    expect(await readHostFile(root, inside)).toBe("after\n");
  },
);

it("stages selected host diff content without replacing the working file", async () => {
  const root = realpathSync.native(
    mkdtempSync(join(tmpdir(), "monocode-workspace-hunk-")),
  );
  roots.push(root);
  const git = (...args: string[]) =>
    execFileSync("git", args, { cwd: root, encoding: "utf8" });
  git("init", "-q");
  git("config", "core.autocrlf", "false");
  git("config", "user.name", "Workspace Test");
  git("config", "user.email", "workspace@example.test");
  mkdirSync(join(root, "src"), { recursive: true });
  writeFileSync(join(root, "src/app.ts"), "one\ntwo\nthree\n");
  git("add", "src/app.ts");
  git("commit", "-qm", "initial");
  writeFileSync(join(root, "src/app.ts"), "ONE\ntwo\nTHREE\n");
  await hostGitAction(
    root,
    "stageContents",
    "src/app.ts",
    undefined,
    "ONE\ntwo\nthree\n",
  );
  expect(git("show", ":src/app.ts")).toBe("ONE\ntwo\nthree\n");
  expect(await readHostFile(root, "src/app.ts")).toBe("ONE\ntwo\nTHREE\n");
  await hostGitAction(root, "discard", "src/app.ts");
  expect(await readHostFile(root, "src/app.ts")).toBe("ONE\ntwo\nthree\n");
  expect(git("ls-files").trim()).toBe("src/app.ts");
  await expect(
    hostGitAction(root, "stageContents", "../escape", undefined, "x"),
  ).rejects.toThrow("outside");
});

function makeRepo(prefix = "monocode-f0-") {
  const root = realpathSync.native(mkdtempSync(join(tmpdir(), prefix)));
  roots.push(root);
  const git = (...args: string[]) =>
    execFileSync("git", args, { cwd: root, encoding: "utf8" });
  git("init", "-q", "-b", "main");
  git("config", "core.autocrlf", "false");
  git("config", "user.name", "Workspace Test");
  git("config", "user.email", "workspace@example.test");
  git("config", "commit.gpgsign", "false");
  return { root, git };
}

async function spawns<T>(run: () => Promise<T>): Promise<[T, number]> {
  resetGitExecStats();
  const value = await run();
  return [value, getGitExecStats().spawned];
}

function withRemote(root: string, git: (...args: string[]) => string) {
  const remote = realpathSync.native(
    mkdtempSync(join(tmpdir(), "monocode-f0-remote-")),
  );
  roots.push(remote);
  execFileSync("git", ["init", "--bare", "-q", "-b", "main"], { cwd: remote });
  git("remote", "add", "origin", remote);
  git("push", "-q", "-u", "origin", "HEAD");
  void root;
}

it("serves a dirty repository with few git processes and caches it", async () => {
  const { root, git } = makeRepo();
  mkdirSync(join(root, "src"));
  writeFileSync(join(root, "src/app.ts"), "before\n");
  git("add", ".");
  git("commit", "-qm", "initial");
  withRemote(root, git);
  writeFileSync(join(root, "src/app.ts"), "after\n");
  mkdirSync(join(root, "src/added"));
  writeFileSync(join(root, "src/added/new.ts"), "new\n");
  invalidateGitIndexCache();

  const [cold, coldSpawns] = await spawns(() => hostGitIndex(root));
  expect(cold).toMatchObject({
    branch: "main",
    remote: "origin",
    upstream: "origin/main",
    defaultBranch: "main",
    ahead: 0,
    behind: 0,
    headPushed: true,
  });
  expect(cold.files.map((file) => [file.relative, file.status])).toEqual([
    ["src/added/new.ts", "untracked"],
    ["src/app.ts", "modified"],
  ]);
  expect(coldSpawns).toBeLessThanOrEqual(8);

  // Within the TTL and with nothing changed, polling spawns nothing.
  const [again, againSpawns] = await spawns(() => hostGitIndex(root));
  expect(again).toBe(cold);
  expect(againSpawns).toBe(0);

  // A fresh edit refreshes the snapshot but reuses the cached remote facts.
  writeFileSync(join(root, "src/app.ts"), "after, longer\n");
  const [warm, warmSpawns] = await spawns(() => hostGitIndex(root));
  expect(warm.files.find((file) => file.relative === "src/app.ts")).toMatchObject(
    { additions: 1, deletions: 1 },
  );
  expect(warmSpawns).toBeLessThanOrEqual(3);

  // Concurrent callers share one run.
  invalidateGitIndexCache();
  const [, single] = await spawns(() => hostGitIndex(root));
  invalidateGitIndexCache();
  const [results, shared] = await spawns(() =>
    Promise.all([1, 2, 3, 4, 5].map(() => hostGitIndex(root))),
  );
  expect(shared).toBe(single);
  expect(new Set(results).size).toBe(1);
});

it("needs one git process for a clean snapshot without sync metadata", async () => {
  const { root, git } = makeRepo();
  writeFileSync(join(root, "a.txt"), "a\n");
  git("add", ".");
  git("commit", "-qm", "initial");
  invalidateGitIndexCache();
  const [index, count] = await spawns(() =>
    hostGitIndex(root, { includeSync: false }),
  );
  expect(index.files).toEqual([]);
  expect(count).toBe(1);
  expect(index.remote).toBeNull();
});

it("notices external git changes without waiting for the TTL", async () => {
  const { root, git } = makeRepo();
  writeFileSync(join(root, "app.ts"), "before\n");
  git("add", ".");
  git("commit", "-qm", "initial");
  writeFileSync(join(root, "app.ts"), "after\n");
  expect((await hostGitIndex(root)).files).toHaveLength(1);
  const headBefore = (await hostGitIndex(root)).head;

  git("add", "app.ts");
  git("commit", "-qm", "second");
  const committed = await hostGitIndex(root);
  expect(committed.files).toEqual([]);
  expect(committed.head).not.toBe(headBefore);

  expect((await hostGitIndex(root)).remote).toBeNull();
  git("remote", "add", "origin", root);
  expect((await hostGitIndex(root)).remote).toBe("origin");

  // A working-tree edit to an already changed file is visible at once.
  writeFileSync(join(root, "app.ts"), "edit\n");
  git("add", "app.ts");
  writeFileSync(join(root, "app.ts"), "edit\nedit two\n");
  const first = await hostGitIndex(root);
  writeFileSync(join(root, "app.ts"), "edit\nedit two\nthree\n");
  const second = await hostGitIndex(root);
  expect(second.files[0].additions).toBeGreaterThan(first.files[0].additions);
});

it("invalidates cached snapshots after host writes", async () => {
  const { root, git } = makeRepo();
  writeFileSync(join(root, "app.ts"), "before\n");
  writeFileSync(join(root, "other.ts"), "other\n");
  git("add", ".");
  git("commit", "-qm", "initial");
  writeFileSync(join(root, "app.ts"), "after\n");
  expect((await hostGitIndex(root)).files[0].staged).toBe(false);
  await hostGitAction(root, "stage", "app.ts");
  expect((await hostGitIndex(root)).files[0].staged).toBe(true);
  await hostGitAction(root, "unstage", "app.ts");
  expect((await hostGitIndex(root)).files[0].staged).toBe(false);
  await hostGitAction(root, "discard", "app.ts");
  expect((await hostGitIndex(root)).files).toEqual([]);
  await writeHostFile(root, "other.ts", "other\n", "changed\n");
  expect((await hostGitIndex(root)).files.map((file) => file.relative)).toEqual([
    "other.ts",
  ]);
  await createHostPath(root, "", "fresh.ts", false);
  expect((await hostGitIndex(root)).files.map((file) => file.relative)).toEqual([
    "fresh.ts",
    "other.ts",
  ]);
  await hostGitAction(root, "discardAll");
  expect((await hostGitIndex(root)).files).toEqual([]);
});

it("diffs one file without a full index, and stats reuse the cache", async () => {
  const { root, git } = makeRepo();
  writeFileSync(join(root, "app.ts"), "before\n");
  git("add", ".");
  git("commit", "-qm", "initial");
  writeFileSync(join(root, "app.ts"), "after\n");
  writeFileSync(join(root, "new.ts"), "new\n");
  invalidateGitIndexCache();

  const [modified, modifiedSpawns] = await spawns(() =>
    hostFileDiff(root, "app.ts", false),
  );
  expect(modified).toMatchObject({ original: "before\n", current: "after\n" });
  expect(modifiedSpawns).toBeLessThanOrEqual(2);

  const [untracked, untrackedSpawns] = await spawns(() =>
    hostFileDiff(root, "new.ts", false),
  );
  expect(untracked).toMatchObject({
    status: "untracked",
    original: "",
    current: "new\n",
  });
  expect(untrackedSpawns).toBe(1);
  await expect(hostFileDiff(root, "missing.ts", false)).rejects.toThrow(
    "no uncommitted changes",
  );

  invalidateGitIndexCache();
  const [stats, coldSpawns] = await spawns(() => hostGitStats(root));
  expect(stats).toEqual({ files: 2, additions: 2, deletions: 1 });
  expect(coldSpawns).toBeLessThanOrEqual(2);
  const [, warmSpawns] = await spawns(() => hostGitStats(root));
  expect(warmSpawns).toBe(0);
  await hostGitIndex(root);
  const [, fullWarm] = await spawns(() => hostGitStats(root));
  expect(fullWarm).toBe(0);
});

it("parses unborn, detached, renamed and awkwardly named entries", async () => {
  const { root, git } = makeRepo();
  writeFileSync(join(root, "first.txt"), "first\n");
  const unborn = await hostGitIndex(root);
  expect(unborn).toMatchObject({ head: null, branch: "main" });
  expect(unborn.files).toMatchObject([{ relative: "first.txt", status: "untracked" }]);

  git("add", ".");
  git("commit", "-qm", "initial");
  git("mv", "first.txt", "second one.txt");
  const renamed = await hostGitIndex(root);
  expect(renamed.files).toMatchObject([
    { relative: "second one.txt", staged: true, unstaged: false },
  ]);
  git("commit", "-qm", "rename");

  const odd = ["ünï cödé.txt", "tab\tname.txt", "new\nline.txt"];
  for (const name of odd) writeFileSync(join(root, name), "x\n");
  const names = (await hostGitIndex(root)).files.map((file) => file.relative);
  expect(names.sort()).toEqual([...odd].sort());

  git("add", ".");
  git("commit", "-qm", "odd");
  git("checkout", "-q", "--detach");
  expect(await hostGitIndex(root)).toMatchObject({ branch: null });
});

it("reports tracked blobs by indexOid, and expands untracked folders within a cap", async () => {
  const { root, git } = makeRepo();
  writeFileSync(join(root, "app.ts"), "before\n");
  writeFileSync(join(root, ".gitignore"), "*.log\n");
  git("add", ".");
  git("commit", "-qm", "initial");
  writeFileSync(join(root, "app.ts"), "after\n");
  mkdirSync(join(root, "pkg"));
  writeFileSync(join(root, "pkg/a.ts"), "a\n");
  writeFileSync(join(root, "pkg/debug.log"), "ignored\n");
  mkdirSync(join(root, "dist"));
  for (let i = 0; i < 250; i++) writeFileSync(join(root, `dist/f${i}.js`), "x\n");

  const index = await hostGitIndex(root);
  const byPath = new Map(index.files.map((file) => [file.relative, file]));
  expect(byPath.get("app.ts")?.indexOid).toMatch(/^[0-9a-f]{40,64}$/);
  expect(byPath.get("pkg/a.ts")?.indexOid).toBeUndefined();
  expect(byPath.get("pkg/a.ts")?.additions).toBe(1);
  expect(byPath.has("pkg/debug.log")).toBe(false);
  // Oversized untracked folders stay collapsed instead of listing every file.
  expect(byPath.get("dist/")).toMatchObject({ status: "untracked", additions: 0 });
  expect(index.files.some((file) => file.relative.startsWith("dist/f"))).toBe(
    false,
  );
});

it("does not skip unignored untracked folders named target when expanding", async () => {
  const { root, git } = makeRepo();
  writeFileSync(join(root, "README.md"), "hi\n");
  git("add", ".");
  git("commit", "-qm", "initial");
  mkdirSync(join(root, "new/target"), { recursive: true });
  writeFileSync(join(root, "new/target/result.txt"), "line1\nline2\n");

  const index = await hostGitIndex(root);
  const byPath = new Map(index.files.map((file) => [file.relative, file]));
  expect(byPath.get("new/target/result.txt")).toMatchObject({
    status: "untracked",
    additions: 2,
  });
});

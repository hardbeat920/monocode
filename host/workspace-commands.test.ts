import { execFileSync } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it } from "vitest";
import { HostStore } from "./store";
import { WorkspaceCommands } from "./workspace-commands";

const cleanups: Array<() => void> = [];
afterEach(() =>
  cleanups
    .splice(0)
    .reverse()
    .forEach((cleanup) => cleanup()),
);

function fixture() {
  // Use the same native resolution as fs/promises.realpath in the host,
  // including expansion of Windows short directory names such as RUNNER~1.
  const home = realpathSync.native(
    mkdtempSync(join(tmpdir(), "monocode-selected-git-")),
  );
  const store = new HostStore(join(home, "host.db"));
  cleanups.push(() => {
    store.close();
    rmSync(home, { recursive: true, force: true });
  });
  const commands = new WorkspaceCommands(store, async (_id, action) =>
    action(),
  );
  const repo = (name = "a", initial = true) => {
    const cwd = join(home, name);
    mkdirSync(cwd, { recursive: true });
    const git = (...args: string[]) =>
      execFileSync("git", args, { cwd, encoding: "utf8" }).trim();
    git("init", "-q", "-b", "main");
    git("config", "user.name", "Test");
    git("config", "user.email", "test@example.com");
    git("config", "commit.gpgsign", "false");
    git("config", "core.autocrlf", "false");
    const write = (file: string, content: string) =>
      writeFileSync(join(cwd, file), content);
    if (initial) {
      for (const file of ["chosen.txt", "other.txt", "deleted.txt"])
        write(file, "old\n");
      git("add", ".");
      git("commit", "-q", "-m", "Initial");
    }
    store.addProject(cwd, name);
    commands.invalidateRoots();
    return { cwd, git, write };
  };
  return { home, commands, repo };
}

const apiRoot = (path: string) =>
  process.platform === "win32" ? path.replace(/\\/g, "/") : path;

it("commits only selected working files and preserves other staging and repos", async () => {
  const { commands, repo } = fixture();
  const a = repo("a"),
    b = repo("b");
  a.write("other.txt", "unrelated staged\n");
  a.git("add", "other.txt");
  a.write("chosen.txt", "partial staging\n");
  a.git("add", "chosen.txt");
  a.write("chosen.txt", "selected work\n");
  b.write("chosen.txt", "second repo work\n");
  const bHead = b.git("rev-parse", "HEAD");
  await commands.run("git_commit", {
    cwd: a.cwd,
    message: "Selected",
    paths: ["chosen.txt"],
  });
  expect(a.git("show", "HEAD:chosen.txt")).toBe("selected work");
  expect(a.git("show", "HEAD:other.txt")).toBe("old");
  expect(a.git("show", ":other.txt")).toBe("unrelated staged");
  expect(a.git("diff", "--cached", "--name-only")).toBe("other.txt");
  expect(b.git("rev-parse", "HEAD")).toBe(bHead);
  expect(readFileSync(join(b.cwd, "chosen.txt"), "utf8")).toBe(
    "second repo work\n",
  );
  await commands.run("git_commit", {
    cwd: a.cwd,
    message: "Existing staged workflow",
  });
  expect(a.git("show", "HEAD:other.txt")).toBe("unrelated staged");
});

it("generates context for selected added and deleted files without reading excluded changes or altering the index", async () => {
  const { commands, repo } = fixture();
  const { cwd, git, write } = repo();
  write("other.txt", "EXCLUDED_SECRET\n");
  write("chosen.txt", "partial staging\n");
  git("add", ".");
  write("chosen.txt", "selected work\n");
  write("new.txt", "new selected work\n");
  rmSync(join(cwd, "deleted.txt"));
  git("add", "deleted.txt");
  const before = readFileSync(join(cwd, ".git/index"));
  const paths = ["chosen.txt", "new.txt", "deleted.txt"];
  const context = (await commands.run("git_staged_context", {
    cwd,
    paths,
  })) as { summary: string; patch: string };
  expect(context.patch).toContain("+selected work");
  expect(context.patch).toContain("+new selected work");
  expect(context.patch).toContain("deleted file mode");
  expect(context.patch).not.toContain("EXCLUDED_SECRET");
  expect(context.summary).not.toContain("other.txt");
  expect(readFileSync(join(cwd, ".git/index"))).toEqual(before);
  await commands.run("git_commit", {
    cwd,
    message: "Selected add and delete",
    paths,
  });
  expect(git("ls-tree", "--name-only", "HEAD").split("\n")).toEqual([
    "chosen.txt",
    "new.txt",
    "other.txt",
  ]);
  expect(git("show", ":other.txt")).toBe("EXCLUDED_SECRET");
});

it("supports the first commit while leaving other staged new files out", async () => {
  const { commands, repo } = fixture();
  const { cwd, git, write } = repo("new", false);
  write("chosen.txt", "chosen\n");
  write("other.txt", "other\n");
  git("add", "other.txt");
  const context = (await commands.run("git_staged_context", {
    cwd,
    paths: ["chosen.txt"],
  })) as { patch: string };
  expect(context.patch).toContain("+chosen");
  await commands.run("git_commit", {
    cwd,
    message: "First",
    paths: ["chosen.txt"],
  });
  expect(git("ls-tree", "--name-only", "HEAD")).toBe("chosen.txt");
  expect(git("diff", "--cached", "--name-only")).toBe("other.txt");
});

it("rejects invalid and cross-repo selections before touching staging", async () => {
  const { commands, repo } = fixture();
  const { cwd, git, write } = repo();
  repo("a/nested");
  mkdirSync(join(cwd, "folder"));
  write("folder/child.txt", "new\n");
  git("add", "folder/child.txt");
  rmSync(join(cwd, "folder"), { recursive: true });
  write("chosen.txt", "unstaged\n");
  const index = readFileSync(join(cwd, ".git/index"));
  for (const invalid of [
    "",
    "../escape",
    ".git/config",
    "./chosen.txt",
    "nested/chosen.txt",
    "folder",
    "missing.txt",
  ]) {
    const args = { cwd, message: "Invalid", paths: ["chosen.txt", invalid] };
    await expect(commands.run("git_commit", args)).rejects.toThrow();
    await expect(commands.run("git_staged_context", args)).rejects.toThrow();
    expect(readFileSync(join(cwd, ".git/index"))).toEqual(index);
  }
  await expect(
    commands.run("git_commit", { cwd, message: "Empty", paths: [] }),
  ).rejects.toThrow();
  await expect(
    commands.run("git_commit", {
      cwd,
      message: "Amend",
      amend: true,
      paths: ["chosen.txt"],
    }),
  ).rejects.toThrow("cannot amend");
});

it.skipIf(process.platform === "win32")(
  "treats wildcard, pathspec, newline and backslash filenames literally",
  async () => {
    const { commands, repo } = fixture();
    const { cwd, git, write } = repo();
    const paths = [
      "*",
      ":(glob)*",
      "-flag",
      "line\nbreak",
      "back\\slash",
      "hello é.txt",
    ];
    for (const path of paths) write(path, "literal\n");
    write("other.txt", "excluded\n");
    await commands.run("git_commit", { cwd, message: "Literal", paths });
    for (const path of paths)
      expect(git("show", `HEAD:${path}`)).toBe("literal");
    expect(git("show", "HEAD:other.txt")).toBe("old");
  },
);

it("locates each checkout and deleted parents while enforcing host boundaries", async () => {
  const { home, commands, repo } = fixture();
  const a = repo("a"),
    b = repo("b");
  repo("a/nested");
  const tree = join(home, "tree");
  a.git("worktree", "add", "-b", "other", tree);
  commands.invalidateRoots();
  const relatives = [
    "deleted/folder/file.txt",
    "chosen.txt",
    "chosen.txt",
    "nested/chosen.txt",
  ];
  const roots = [a.cwd, b.cwd, tree, a.cwd];
  expect(
    await commands.run("git_locate_files", {
      paths: relatives.map((path, i) => join(roots[i], path)),
    }),
  ).toEqual([
    { root: apiRoot(a.cwd), relative: relatives[0] },
    { root: apiRoot(b.cwd), relative: "chosen.txt" },
    { root: apiRoot(tree), relative: "chosen.txt" },
    { root: apiRoot(join(a.cwd, "nested")), relative: "chosen.txt" },
  ]);
  await expect(
    commands.run("git_locate_files", { paths: [join(home, "outside.txt")] }),
  ).rejects.toThrow();
  const outside = join(home, "non-git");
  mkdirSync(outside);
  const store = new HostStore(join(outside, "host.db"));
  cleanups.push(() => store.close());
  store.addProject(outside, "non-git");
  const nonGit = new WorkspaceCommands(store, async (_id, action) => action());
  expect(
    await nonGit.run("git_locate_files", {
      paths: [join(outside, "file.txt")],
    }),
  ).toEqual([null]);
});

it.skipIf(process.platform === "win32")(
  "keeps file symlinks in their checkout and rejects traversal through outside directories",
  async () => {
    const { home, commands, repo } = fixture();
    const { cwd } = repo();
    writeFileSync(join(home, "secret.txt"), "OUTSIDE_SECRET\n");
    symlinkSync(join(home, "secret.txt"), join(cwd, "file-link"));
    symlinkSync(home, join(cwd, "dir-link"));
    expect(
      await commands.run("git_locate_files", {
        paths: [join(cwd, "file-link")],
      }),
    ).toEqual([{ root: apiRoot(cwd), relative: "file-link" }]);
    const context = (await commands.run("git_staged_context", {
      cwd,
      paths: ["file-link"],
    })) as { patch: string };
    expect(context.patch).toContain("new file mode 120000");
    expect(context.patch).not.toContain("OUTSIDE_SECRET");
    await expect(
      commands.run("git_staged_context", {
        cwd,
        paths: ["dir-link/secret.txt"],
      }),
    ).rejects.toThrow();
    await expect(
      commands.run("git_commit", {
        cwd,
        message: "Escape",
        paths: ["dir-link/secret.txt"],
      }),
    ).rejects.toThrow();
  },
);

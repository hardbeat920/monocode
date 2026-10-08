import { execFileSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import {
  GitHostCheckouts,
  hostCheckout,
  planHostCheckout,
  type GitHostProvider,
} from "./git-hosts";
import { parseGithubRepos } from "./git-hosts-github";

const cleanups: string[] = [];
afterEach(() => {
  for (const dir of cleanups.splice(0)) rmSync(dir, { recursive: true, force: true });
});

const parent = () => {
  const dir = mkdtempSync(join(tmpdir(), "monocode-git-hosts-"));
  cleanups.push(dir);
  return dir;
};

/** A checkout with one commit, like a finished clone; `commit: false` leaves
 * it like an interrupted one. */
const repo = (path: string, remote: string, commit = true) => {
  mkdirSync(path, { recursive: true });
  execFileSync("git", ["init", "-q"], { cwd: path });
  execFileSync("git", ["remote", "add", "origin", remote], { cwd: path });
  if (commit)
    execFileSync(
      "git",
      ["-c", "user.name=Test", "-c", "user.email=test@example.com", "commit", "-q", "--allow-empty", "-m", "initial"],
      { cwd: path },
    );
};

const fake = (cloneInto = vi.fn(async (_slug: string, dest: string) => {
  mkdirSync(dest, { recursive: true });
})): GitHostProvider & { cloneInto: typeof cloneInto } => ({
  id: "github",
  domain: "github.com",
  status: async () => ({ provider: "github", installed: true, authenticated: true }),
  repos: async () => [],
  cloneInto,
});

it("clones into the repository name when it is free", async () => {
  const root = parent();
  const provider = fake();
  expect(await hostCheckout(provider, "owner/repo", root)).toEqual({ path: join(root, "repo"), reuse: false });
  expect(provider.cloneInto).toHaveBeenCalledWith("owner/repo", join(root, "repo"));
});

it("reuses a folder that already tracks the repository", async () => {
  const root = parent();
  repo(join(root, "repo"), "git@github.com:Owner/Repo.git");
  const provider = fake();
  expect(await hostCheckout(provider, "owner/repo", root)).toEqual({ path: join(root, "repo"), reuse: true });
  expect(provider.cloneInto).not.toHaveBeenCalled();
});

it("steps past folders that hold something else", async () => {
  const root = parent();
  repo(join(root, "repo"), "https://github.com/someone/else.git");
  mkdirSync(join(root, "repo-2"));
  writeFileSync(join(root, "repo-2", "notes.txt"), "x");
  expect(await planHostCheckout(fake(), "owner/repo", root)).toEqual({ path: join(root, "repo-3"), reuse: false });
});

it("does not reuse an interrupted clone", async () => {
  const root = parent();
  repo(join(root, "repo"), "https://github.com/owner/repo", false);
  expect(await planHostCheckout(fake(), "owner/repo", root)).toEqual({ path: join(root, "repo-2"), reuse: false });
});

it.skipIf(process.platform === "win32" || process.getuid?.() === 0)(
  "treats an unreadable folder as taken",
  async () => {
    const root = parent();
    mkdirSync(join(root, "repo"), { mode: 0o000 });
    try {
      expect(await planHostCheckout(fake(), "owner/repo", root)).toEqual({ path: join(root, "repo-2"), reuse: false });
    } finally {
      chmodSync(join(root, "repo"), 0o700);
    }
  },
);

it("refuses a second clone into a folder still being cloned", async () => {
  const root = parent();
  let finish!: () => void;
  const provider = fake(vi.fn(async (_slug: string, dest: string) => {
    mkdirSync(dest);
    await new Promise<void>((resolve) => (finish = resolve));
  }));
  const first = hostCheckout(provider, "owner/repo", root);
  await vi.waitFor(() => expect(provider.cloneInto).toHaveBeenCalled());
  await expect(planHostCheckout(provider, "owner/repo", root)).rejects.toThrow("Already cloning");
  finish();
  await first;
});

it("rejects unsafe input", async () => {
  const root = parent();
  await expect(planHostCheckout(fake(), "../etc", root)).rejects.toThrow("owner/name");
  await expect(planHostCheckout(fake(), "owner/..git", root)).rejects.toThrow("owner/name");
  await expect(planHostCheckout(fake(), "owner/repo", "relative")).rejects.toThrow("absolute");
  await expect(planHostCheckout(fake(), "owner/repo", join(root, "missing"))).rejects.toThrow("not a folder");
});

it("runs checkouts as jobs and opens the project when done", async () => {
  const root = parent();
  let finish!: () => void;
  const provider = fake(vi.fn(async (_slug: string, dest: string) => {
    await new Promise<void>((resolve) => (finish = resolve));
    mkdirSync(dest);
  }));
  const open = vi.fn(async (cwd: string) => ({ id: "p1", cwd, name: "repo" }));
  const checkouts = new GitHostCheckouts(open, { github: provider });
  const params = { provider: "github", slug: "owner/repo", parent: root };
  const { jobId } = checkouts.start(params);
  expect(checkouts.start(params).jobId).toBe(jobId);
  await vi.waitFor(() => expect(provider.cloneInto).toHaveBeenCalled());
  expect(checkouts.status(jobId)).toEqual({ state: "running" });
  finish();
  await vi.waitFor(() => expect(checkouts.status(jobId).state).toBe("done"));
  expect(checkouts.status(jobId)).toEqual({
    state: "done",
    reused: false,
    project: { id: "p1", cwd: join(root, "repo"), name: "repo" },
  });
});

it("reports failed checkouts", async () => {
  const provider = fake(vi.fn(async () => {
    throw new Error("Repository not found");
  }));
  const checkouts = new GitHostCheckouts(vi.fn(), { github: provider });
  const { jobId } = checkouts.start({ provider: "github", slug: "owner/repo", parent: parent() });
  await vi.waitFor(() => expect(checkouts.status(jobId)).toEqual({ state: "error", error: "Repository not found" }));
  expect(() => checkouts.start({ provider: "gitlab" })).toThrow("Unknown Git host");
});

it("parses GitHub repository listings", () => {
  expect(parseGithubRepos(
    '{"slug":"o/a","description":"","private":true,"pushedAt":"2026-01-01T00:00:00Z"}\n' +
    '{"slug":"o/b","description":"B","private":false,"pushedAt":null}\n',
  )).toEqual([
    { provider: "github", slug: "o/a", description: undefined, private: true, pushedAt: "2026-01-01T00:00:00Z" },
    { provider: "github", slug: "o/b", description: "B", private: false, pushedAt: undefined },
  ]);
});

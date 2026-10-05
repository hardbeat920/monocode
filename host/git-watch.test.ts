import { afterEach, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  realpathSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { GitWatchHost } from "./git-watch";

const cleanups: Array<() => void> = [];
afterEach(() => {
  for (const cleanup of cleanups.splice(0).reverse()) cleanup();
});
function setup() {
  const root = realpathSync(
    mkdtempSync(join(tmpdir(), "monocode-host-watch-")),
  );
  cleanups.push(() => rmSync(root, { recursive: true, force: true }));
  const git = (...args: string[]) =>
    execFileSync("git", ["-C", root, ...args], { stdio: "pipe" });
  git("init", "-q", "-b", "main");
  writeFileSync(join(root, "file.txt"), "initial\n");
  writeFileSync(join(root, ".gitignore"), "build/\n");
  mkdirSync(join(root, "build"));
  writeFileSync(join(root, "build/generated.txt"), "ignored");
  git("add", ".");
  git(
    "-c",
    "user.name=Test",
    "-c",
    "user.email=test@example.com",
    "-c",
    "commit.gpgsign=false",
    "commit",
    "-qm",
    "initial",
  );
  const host = new GitWatchHost();
  cleanups.push(() => host.close());
  return { root, git, host };
}
const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
async function changed(promise: Promise<boolean>) {
  // Fail quickly and cleanly if an OS notification was missed.
  const result = await Promise.race([
    promise,
    delay(2000).then(() => "missed"),
  ]);
  expect(result).toBe(true);
}

it("reports external atomic edits, staging, branch changes, and deletions", async () => {
  const { root, git, host } = setup();
  await host.start("watch", root, root);
  const edit = host.wait("watch", root);
  writeFileSync(join(root, "file.tmp"), "agent edit\n");
  renameSync(join(root, "file.tmp"), join(root, "file.txt"));
  await changed(edit);
  const staged = host.wait("watch", root);
  git("add", "file.txt");
  await changed(staged);
  const branch = host.wait("watch", root);
  git("checkout", "-qb", "agent-branch");
  await changed(branch);
  const deleted = host.wait("watch", root);
  rmSync(join(root, "file.txt"));
  await changed(deleted);
  host.stop("watch", root);
});

it("stays quiet on Git reads and ignored builds, and releases a pending request on close", async () => {
  const { root, git, host } = setup();
  await host.start("watch", root, root);
  let refreshed = false;
  const pending = host.wait("watch", root).then((changed) => {
    refreshed = changed;
    return changed;
  });
  git("--no-optional-locks", "status", "--porcelain");
  git("diff");
  writeFileSync(join(root, "build/generated.txt"), "build output");
  writeFileSync(join(root, "build/new.txt"), "more output");
  await delay(350);
  expect(refreshed).toBe(false);
  host.stop("watch", root);
  expect(await pending).toBe(false);
  expect(() => host.wait("watch", root)).toThrow("not found");
});

it("keeps changes received between long requests and rejects a different repository key", async () => {
  const { root, host } = setup();
  await host.start("watch", root, root);
  expect(() => host.wait("watch", "/another-project")).toThrow("not found");
  writeFileSync(join(root, "new.txt"), "new file");
  await delay(200);
  await changed(host.wait("watch", root));
});

it("watches the shared Git metadata of linked worktrees", async () => {
  const { root, git, host } = setup();
  const linked = join(root, "linked");
  git("worktree", "add", "-qb", "linked", linked);
  await host.start("watch", linked, linked);
  const refs = host.wait("watch", linked);
  git("branch", "external-branch");
  await changed(refs);
});

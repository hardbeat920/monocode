import { watch, type FSWatcher } from "node:fs";
import { stat } from "node:fs/promises";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import {
  basename,
  dirname,
  isAbsolute,
  relative,
  resolve,
  sep,
} from "node:path";

const exec = promisify(execFile);
const BATCH_MS = 100;
const WAIT_MS = 20_000;
const LEASE_MS = 60_000;
const within = (path: string, root: string) => {
  const rel = relative(root, path);
  return (
    rel === "" ||
    (rel !== ".." && !rel.startsWith(`..${sep}`) && !isAbsolute(rel))
  );
};
const slashed = (path: string) => path.split(sep).join("/");
const metadataRelevant = (path: string) =>
  !path.endsWith(".lock") &&
  (path === "" ||
    [
      "HEAD",
      "index",
      "config",
      "config.worktree",
      "packed-refs",
      "shallow",
      "info/exclude",
    ].includes(path) ||
    path === "refs" ||
    path.startsWith("refs/"));

async function git(root: string, args: string[]) {
  try {
    return (
      await exec("git", ["-C", root, ...args], {
        env: {
          ...process.env,
          GIT_OPTIONAL_LOCKS: "0",
          GIT_TERMINAL_PROMPT: "0",
        },
        maxBuffer: 16 * 1024 * 1024,
        timeout: 10_000,
      })
    ).stdout;
  } catch {
    return "";
  }
}

class RepositoryWatch {
  private watchers: FSWatcher[] = [];
  private gitDirs: string[] = [];
  private ignored = new Set<string>();
  private tracked = new Set<string>();
  private paths = new Set<string>();
  private rescan = false;
  private reloadRules = false;
  private timer?: ReturnType<typeof setTimeout>;
  private lease?: ReturnType<typeof setTimeout>;
  private flushing = false;
  private dirty = false;
  private closed = false;
  private error?: Error;
  private waiter?: {
    resolve: (changed: boolean) => void;
    reject: (error: Error) => void;
    timer: ReturnType<typeof setTimeout>;
  };

  constructor(
    readonly cwd: string,
    readonly key: string,
    private readonly expire: () => void,
  ) {}

  async start() {
    const dirs = (
      await git(this.cwd, [
        "rev-parse",
        "--path-format=absolute",
        "--git-dir",
        "--git-common-dir",
      ])
    )
      .trim()
      .split("\n")
      .filter(Boolean);
    this.gitDirs = [...new Set([...dirs, resolve(this.cwd, ".git")])].sort(
      (a, b) => b.length - a.length,
    );
    const roots = [this.cwd];
    for (const dir of this.gitDirs) {
      if (
        roots.some((parent) => within(dir, parent)) ||
        !(await stat(dir).catch(() => null))?.isDirectory()
      )
        continue;
      for (let i = roots.length - 1; i >= 0; i--)
        if (within(roots[i], dir)) roots.splice(i, 1);
      roots.push(dir);
    }
    try {
      if (this.closed) throw new Error("Git watcher closed during setup");
      for (const root of roots) {
        const watcher = watch(
          root,
          { recursive: true, persistent: false },
          (kind, name) => {
            if (this.closed) return;
            if (name === null) this.rescan = true;
            else {
              const path = resolve(root, name.toString());
              if (!this.relevant(path)) return;
              this.paths.add(path);
              const meta = this.metadata(path);
              if (
                basename(path) === ".gitignore" ||
                basename(path) === ".git" ||
                (meta !== undefined &&
                  [
                    "index",
                    "config",
                    "config.worktree",
                    "info/exclude",
                  ].includes(meta)) ||
                (meta === undefined && kind === "rename")
              )
                this.reloadRules = true;
            }
            this.schedule();
          },
        );
        watcher.on("error", (error) => this.fail(error));
        this.watchers.push(watcher);
      }
      await this.loadIgnored();
      if (this.error) throw this.error;
      if (this.closed) throw new Error("Git watcher closed during setup");
      this.renew();
    } catch (error) {
      this.close();
      throw error;
    }
  }

  private metadata(path: string) {
    const dir = this.gitDirs.find((dir) => within(path, dir));
    return dir === undefined ? undefined : slashed(relative(dir, path));
  }

  private relevant(path: string): boolean {
    const meta = this.metadata(path);
    if (meta !== undefined) return metadataRelevant(meta);
    if (
      !within(path, this.cwd) ||
      relative(this.cwd, path).split(sep).includes(".git")
    )
      return false;
    if (this.tracked.has(path)) return true;
    for (
      let parent = path;
      within(parent, this.cwd);
      parent = dirname(parent)
    ) {
      if (this.ignored.has(parent)) return false;
      if (parent === this.cwd) break;
    }
    return true;
  }

  private async loadIgnored() {
    const [ignored, tracked] = await Promise.all([
      git(this.cwd, [
        "ls-files",
        "--others",
        "--ignored",
        "--exclude-standard",
        "--directory",
        "-z",
        "--",
        ".",
      ]),
      git(this.cwd, ["ls-files", "--cached", "-z", "--", "."]),
    ]);
    const paths = (output: string) =>
      new Set(
        output
          .split("\0")
          .filter(Boolean)
          .map((path) => resolve(this.cwd, path)),
      );
    this.ignored = paths(ignored);
    this.tracked = paths(tracked);
  }

  private schedule() {
    if (!this.timer && !this.flushing)
      this.timer = setTimeout(() => void this.flush(), BATCH_MS).unref();
  }

  private async flush() {
    this.timer = undefined;
    this.flushing = true;
    const paths = this.paths;
    this.paths = new Set();
    const rescan = this.rescan;
    this.rescan = false;
    const reload = this.reloadRules;
    this.reloadRules = false;
    if (reload || rescan) await this.loadIgnored();
    this.flushing = false;
    if (this.closed) return;
    if (rescan || [...paths].some((path) => this.relevant(path))) {
      this.dirty = true;
      if (this.waiter) this.finishWait(true);
    }
    if (this.rescan || this.paths.size) this.schedule();
  }

  private renew() {
    clearTimeout(this.lease);
    this.lease = setTimeout(this.expire, LEASE_MS).unref();
  }

  wait(): Promise<boolean> {
    if (this.error) return Promise.reject(this.error);
    if (this.closed) return Promise.resolve(false);
    if (this.waiter)
      return Promise.reject(
        new Error("Git watcher already has a pending request"),
      );
    this.renew();
    if (this.dirty) {
      this.dirty = false;
      return Promise.resolve(true);
    }
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => this.finishWait(false), WAIT_MS).unref();
      this.waiter = { resolve, reject, timer };
    });
  }

  private finishWait(changed: boolean) {
    const waiter = this.waiter;
    this.waiter = undefined;
    if (!waiter) return;
    clearTimeout(waiter.timer);
    if (changed) this.dirty = false;
    waiter.resolve(changed);
  }

  private fail(error: Error) {
    this.error = error;
    if (this.waiter) {
      clearTimeout(this.waiter.timer);
      this.waiter.reject(error);
      this.waiter = undefined;
    }
    this.close();
    this.expire();
  }

  close() {
    this.closed = true;
    clearTimeout(this.timer);
    clearTimeout(this.lease);
    for (const watcher of this.watchers) watcher.close();
    this.watchers = [];
    this.finishWait(false);
  }
}

/** Long requests wait for OS events; renewing a request performs no Git reads.
 * Leases release watchers if a client disconnects without unsubscribing. */
export class GitWatchHost {
  private watches = new Map<string, RepositoryWatch>();

  async start(id: unknown, key: unknown, cwd: string): Promise<void> {
    if (
      typeof id !== "string" ||
      !id ||
      id.length > 128 ||
      typeof key !== "string"
    )
      throw new Error("Invalid Git watcher");
    if (this.watches.has(id)) throw new Error("Git watcher already exists");
    if (this.watches.size >= 128) throw new Error("Too many Git watchers");
    const watch = new RepositoryWatch(cwd, key, () => {
      if (this.watches.get(id) === watch) {
        this.watches.delete(id);
        watch.close();
      }
    });
    this.watches.set(id, watch);
    try {
      await watch.start();
    } catch (error) {
      this.watches.delete(id);
      throw error;
    }
  }

  private get(id: unknown, key: unknown) {
    const watch = typeof id === "string" ? this.watches.get(id) : undefined;
    if (!watch || watch.key !== key) throw new Error("Git watcher not found");
    return watch;
  }

  wait(id: unknown, key: unknown) {
    return this.get(id, key).wait();
  }

  stop(id: unknown, key: unknown) {
    const watch = this.get(id, key);
    this.watches.delete(id as string);
    watch.close();
  }

  close() {
    for (const watch of this.watches.values()) watch.close();
    this.watches.clear();
  }
}

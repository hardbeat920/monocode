import { execFile, execFileSync, spawn } from "node:child_process";
import { closeSync, openSync, readSync, statSync } from "node:fs";
import { delimiter, join } from "node:path";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

/**
 * At most this many asynchronous git children run at once inside one host
 * process. (Synchronous helpers like `execGitSync` cannot await the semaphore.)
 */
export const MAX_CONCURRENT_GIT = 4;

let resolvedGit: string | undefined;

/** True for a regular executable file that is not a `#!` script wrapper. */
export function isExecutableNativeBinary(path: string): boolean {
  try {
    const info = statSync(path);
    if (!info.isFile()) return false;
    if (process.platform !== "win32" && (info.mode & 0o111) === 0) return false;
    const fd = openSync(path, "r");
    try {
      const head = Buffer.alloc(2);
      const read = readSync(fd, head, 0, 2, 0);
      return !(read === 2 && head[0] === 0x23 && head[1] === 0x21);
    } finally {
      closeSync(fd);
    }
  } catch {
    return false;
  }
}

function resolvePosixPathGit(): string {
  for (const dir of (process.env.PATH ?? "").split(delimiter)) {
    if (!dir) continue;
    const candidate = join(dir, "git");
    if (isExecutableNativeBinary(candidate)) return candidate;
  }
  return "git";
}

function resolveDarwinGit(): string {
  for (const candidate of ["/opt/homebrew/bin/git", "/usr/local/bin/git"])
    if (isExecutableNativeBinary(candidate)) return candidate;
  try {
    // /usr/bin/git is an Xcode shim that re-execs the real binary; ask for it.
    const candidate = execFileSync("/usr/bin/xcrun", ["-f", "git"], {
      encoding: "utf8",
      timeout: 2000,
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
    if (
      candidate &&
      candidate !== "/usr/bin/git" &&
      isExecutableNativeBinary(candidate)
    )
      return candidate;
  } catch {
    /* no Xcode toolchain: fall through */
  }
  return isExecutableNativeBinary("/usr/bin/git") ? "/usr/bin/git" : "git";
}

/**
 * The real git binary, bypassing `#!` wrappers on PATH (and on macOS, the
 * `/usr/bin/git` Xcode shim) so one logical git call is one exec. Resolved
 * once per process; `MONOCODE_GIT_BIN` always takes precedence.
 */
export function resolveGitBinary(): string {
  if (resolvedGit) return resolvedGit;
  const override = process.env.MONOCODE_GIT_BIN;
  if (override) return (resolvedGit = override);
  resolvedGit =
    process.platform === "darwin"
      ? resolveDarwinGit()
      : process.platform === "win32"
        ? "git"
        : resolvePosixPathGit();
  return resolvedGit;
}

export function __resetGitBinaryForTest(): void {
  resolvedGit = undefined;
}

let active = 0;
const waiting: (() => void)[] = [];
let spawned = 0;
let peak = 0;

/** FIFO semaphore capping concurrent git children. */
export async function withGitSlot<T>(fn: () => Promise<T>): Promise<T> {
  if (active >= MAX_CONCURRENT_GIT)
    await new Promise<void>((resolve) => waiting.push(resolve));
  else active++;
  peak = Math.max(peak, active);
  try {
    return await fn();
  } finally {
    const next = waiting.shift();
    if (next) next();
    else active--;
  }
}

export function getGitExecStats(): { spawned: number; peak: number } {
  return { spawned, peak };
}

export function resetGitExecStats(): void {
  spawned = 0;
  peak = 0;
}

export type ExecGitOptions = {
  /** Skip optional index refresh writes (`--no-optional-locks`). Default true. */
  readOnly?: boolean;
  timeout?: number;
  /** Optional wall-clock deadline (`Date.now()` ms); expired queued calls abort before spawning. */
  deadline?: number;
  maxBuffer?: number;
  env?: NodeJS.ProcessEnv;
};

function gitEnv(readOnly: boolean, env?: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  return {
    ...process.env,
    ...env,
    LC_ALL: "C",
    GIT_TERMINAL_PROMPT: "0",
    ...(readOnly ? { GIT_OPTIONAL_LOCKS: "0" } : {}),
  };
}

function gitArgs(args: string[], readOnly: boolean): string[] {
  return [
    ...(readOnly ? ["--no-optional-locks"] : []),
    "-c",
    "core.pager=cat",
    ...args,
  ];
}

export function execGit(
  cwd: string,
  args: string[],
  opts: ExecGitOptions = {},
): Promise<string> {
  const readOnly = opts.readOnly !== false;
  return withGitSlot(async () => {
    let timeout = opts.timeout ?? 10_000;
    if (opts.deadline !== undefined) {
      const remaining = opts.deadline - Date.now();
      if (remaining <= 0) throw new Error("git deadline exceeded before spawn");
      timeout = Math.min(timeout, remaining);
    }
    spawned++;
    const result = await execFileAsync(resolveGitBinary(), gitArgs(args, readOnly), {
      cwd,
      timeout,
      maxBuffer: opts.maxBuffer ?? 4 * 1024 * 1024,
      encoding: "utf8",
      env: gitEnv(readOnly, opts.env),
    });
    return result.stdout;
  });
}

/** Like {@link execGit} but returns raw stdout bytes (e.g. for `git show`). */
export function execGitBuffer(
  cwd: string,
  args: string[],
  opts: ExecGitOptions = {},
): Promise<Buffer> {
  const readOnly = opts.readOnly !== false;
  return withGitSlot(async () => {
    spawned++;
    const result = await execFileAsync(resolveGitBinary(), gitArgs(args, readOnly), {
      cwd,
      timeout: opts.timeout ?? 10_000,
      maxBuffer: opts.maxBuffer ?? 4 * 1024 * 1024,
      encoding: "buffer",
      env: gitEnv(readOnly, opts.env),
    });
    return Buffer.isBuffer(result.stdout)
      ? result.stdout
      : Buffer.from(result.stdout);
  });
}

export function execGitSync(
  cwd: string,
  args: string[],
  opts: ExecGitOptions = {},
): string {
  const readOnly = opts.readOnly !== false;
  spawned++;
  return execFileSync(resolveGitBinary(), gitArgs(args, readOnly), {
    cwd,
    timeout: opts.timeout ?? 10_000,
    maxBuffer: opts.maxBuffer ?? 1024 * 1024,
    encoding: "utf8",
    env: gitEnv(readOnly, opts.env),
  });
}

/** Run a git command feeding `input` on stdin (a write unless `readOnly`). */
export function spawnGitWithInput(
  cwd: string,
  args: string[],
  input: string,
  readOnly = false,
): Promise<string> {
  return withGitSlot(
    () =>
      new Promise<string>((resolve, reject) => {
        spawned++;
        const child = spawn(resolveGitBinary(), gitArgs(args, readOnly), {
          cwd,
          stdio: ["pipe", "pipe", "pipe"],
          env: gitEnv(readOnly),
        });
        const stdout: Buffer[] = [];
        const stderr: Buffer[] = [];
        let stdinError: Error | null = null;
        let settled = false;
        const finish = (fn: () => void) => {
          if (settled) return;
          settled = true;
          clearTimeout(timer);
          fn();
        };
        const timer = setTimeout(() => {
          child.kill();
        }, 10_000);
        child.stdout.on("data", (chunk: Buffer) => stdout.push(chunk));
        child.stderr.on("data", (chunk: Buffer) => stderr.push(chunk));
        // Hold the concurrency slot until the child actually exits; record any
        // stdin write error and settle on `close` instead of rejecting early.
        child.stdin.on("error", (error: NodeJS.ErrnoException) => {
          if (error.code === "EPIPE") return;
          stdinError = error;
          child.kill();
        });
        child.on("error", (error) => {
          finish(() => reject(error));
        });
        child.on("close", (code) => {
          finish(() => {
            if (stdinError) {
              reject(stdinError);
              return;
            }
            if (code === 0) resolve(Buffer.concat(stdout).toString("utf8"));
            else {
              const err = new Error(
                Buffer.concat(stderr).toString("utf8") || `git exited ${code}`,
              ) as Error & { code?: number | null };
              err.code = code;
              reject(err);
            }
          });
        });
        child.stdin.end(input);
      }),
  );
}

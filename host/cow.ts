import { spawn } from "node:child_process";
import { existsSync, lstatSync, realpathSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { HostStore } from "./store";
import { resolveHostWorktreeAsync } from "./git-worktrees";

export type HostCow = {
  id: string;
  path: string;
  projectCwd: string;
  sourceCwd: string;
  sessionId: string;
  branch: string;
  head: string;
  missing?: boolean;
  rootIdentity?: [string, string];
  gitIdentity?: [string, string];
};

const filename =
  process.platform === "win32"
    ? "monocode-isolation.exe"
    : "monocode-isolation";
const directory = dirname(fileURLToPath(import.meta.url));
const candidates = [
  join(directory, filename),
  ...(directory === resolve("host")
    ? [resolve("target/release", filename), resolve("target/debug", filename)]
    : []),
];

export function cowHelper(): string | undefined {
  return process.platform === "darwin" ? candidates.find(existsSync) : undefined;
}

function result<T>(output: string): T {
  const value = JSON.parse(output) as {
    ok: boolean;
    result?: T;
    error?: string;
  };
  if (!value.ok)
    throw new Error(value.error || "Copy-on-write operation failed");
  return value.result as T;
}

export function hostCow<T>(
  store: HostStore,
  command: string,
  args: Record<string, unknown>,
): Promise<T> {
  const helper = cowHelper();
  if (!helper) {
    if (command === "cow_capability")
      return Promise.resolve({
        supported: false,
        reason:
          "Copy-on-write requires APFS and an updated native MonoCode Host on macOS.",
      } as T);
    if (command === "cow_list" || command === "cow_roots")
      return Promise.resolve([] as T);
    if (command === "cow_owner" && args.path !== undefined)
      return Promise.resolve(null as T);
    return Promise.reject(
      new Error("Copy-on-write requires APFS and an updated native MonoCode Host on macOS."),
    );
  }
  return new Promise((accept, reject) => {
    const child = spawn(helper, ["--store", store.isolationDir], {
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    const output: Buffer[] = [];
    const errors: Buffer[] = [];
    let bytes = 0;
    let failure: Error | undefined;
    // A removal caller may restore sessions on failure. Do not report failure
    // while the helper can still mutate the checkout.
    const stop = (error: Error) => {
      failure ??= error;
      child.kill();
    };
    const timer = setTimeout(() => {
      stop(
        new Error(
          "Copy-on-write operation timed out; its recovery record was retained.",
        ),
      );
    }, 10 * 60_000);
    child.stdout.on("data", (chunk: Buffer) => {
      bytes += chunk.length;
      if (bytes > 16 * 1024 * 1024) {
        stop(new Error("Copy-on-write response is too large."));
      } else output.push(chunk);
    });
    child.stderr.on("data", (chunk: Buffer) => {
      if (errors.length < 64) errors.push(chunk);
    });
    child.on("error", (error) => {
      clearTimeout(timer);
      if (child.pid == null) reject(error); // Spawn failed; no helper exists.
      else failure ??= error; // Failed termination still requires confirmed exit.
    });
    child.stdin.on("error", (error) => {
      stop(error);
    });
    child.on("close", () => {
      clearTimeout(timer);
      if (failure) {
        reject(failure);
        return;
      }
      try {
        accept(result<T>(Buffer.concat(output).toString("utf8")));
      } catch (error) {
        reject(
          output.length
            ? error
            : new Error(
                Buffer.concat(errors).toString("utf8") ||
                  "Copy-on-write helper exited without a response.",
              ),
        );
      }
    });
    child.stdin.end(JSON.stringify({ command, args }));
  });
}

function requestedPath(requested: unknown): string | undefined {
  if (requested == null || requested === "") return undefined;
  if (
    typeof requested !== "string" ||
    requested.includes("\0") ||
    requested.length > 4096
  )
    throw new Error("Invalid working copy");
  return realpathSync.native(requested);
}

/** Cheap ownership recheck after async authorization, before starting a turn. */
export function assertHostCowIdentity(copy: HostCow): void {
  for (const [path, identity] of [
    [copy.path, copy.rootIdentity],
    [join(copy.path, ".git"), copy.gitIdentity],
  ] as const) {
    if (!identity) throw new Error("Update MonoCode Host to verify copy-on-write ownership");
    const actual = lstatSync(path, { bigint: true });
    if (!actual.isDirectory() || actual.isSymbolicLink() ||
      actual.dev.toString() !== identity[0] || actual.ino.toString() !== identity[1])
      throw new Error("Copy-on-write ownership changed");
  }
}

export async function resolveHostWorkspaceAsync(
  store: HostStore,
  projectCwd: string,
  requested: unknown,
): Promise<string> {
  const actual = requestedPath(requested);
  if (!actual || actual === projectCwd) return projectCwd;
  try {
    return await resolveHostWorktreeAsync(projectCwd, actual);
  } catch (error) {
    const copy = await hostCow<HostCow | null>(store, "cow_owner", {
      cwd: projectCwd, path: actual,
    });
    if (!copy) throw error;
    assertHostCowIdentity(copy);
    return copy.path;
  }
}

export async function ownedHostCow(
  store: HostStore,
  projectCwd: string,
  id: string,
): Promise<HostCow> {
  const copy = await hostCow<HostCow>(store, "cow_owner", {
    cwd: projectCwd, cowId: id,
  });
  if (!copy || copy.id !== id || copy.projectCwd !== realpathSync.native(projectCwd))
    throw new Error("This project’s copy-on-write workspace is unavailable.");
  assertHostCowIdentity(copy);
  return copy;
}

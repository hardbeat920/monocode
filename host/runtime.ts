import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  realpathSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { delimiter, dirname, isAbsolute, join, relative, resolve } from "node:path";

/** Files the host bundle needs at run time; see host/build.mjs. */
const BUNDLE = ["monocode-host.mjs", "provider-guard.mjs"];

export type InstalledRuntime = {
  /** The Node executable that runs the host. */
  executable: string;
  /** The host entry point inside the data directory. */
  entry: string;
  /** A stable command for managing the host, such as `~/.monocode-host/bin/monocode-host`. */
  launcher: string;
};

const shellQuote = (value: string) => `'${value.replaceAll("'", "'\\''")}'`;

/**
 * Copies the running host bundle to `<data dir>/runtime/<version>`. `npx`
 * runs packages from a cache that npm may clear at any time, so the
 * background service must not point into it.
 */
export function installRuntime(options: {
  directory: string;
  version: string;
  source: string;
  executable: string;
  platform?: NodeJS.Platform;
}): InstalledRuntime {
  const platform = options.platform ?? process.platform;
  const runtimes = join(options.directory, "runtime");
  const target = join(runtimes, options.version);
  // Running `connect` from an installed runtime reuses it in place.
  if (resolve(options.source) !== resolve(target)) {
    mkdirSync(runtimes, { recursive: true, mode: 0o700 });
    // Copy into a fresh folder and rename it into place, so a concurrent
    // install never leaves a half-copied runtime behind.
    const staging = join(runtimes, `.${options.version}-${process.pid}-${Date.now()}`);
    mkdirSync(staging, { mode: 0o700 });
    try {
      for (const file of [...BUNDLE, "monocode-host.mjs.map"]) {
        const from = join(options.source, file);
        if (existsSync(from)) copyFileSync(from, join(staging, file));
        else if (BUNDLE.includes(file))
          throw new Error(`The host package is missing ${file}`);
      }
      rmSync(target, { recursive: true, force: true });
      renameSync(staging, target);
    } catch (error) {
      rmSync(staging, { recursive: true, force: true });
      throw error;
    }
  }
  const entry = join(target, "monocode-host.mjs");
  const bin = join(options.directory, "bin");
  mkdirSync(bin, { recursive: true, mode: 0o700 });
  const windows = platform === "win32";
  const launcher = join(bin, windows ? "monocode-host.cmd" : "monocode-host");
  writeFileSync(
    `${launcher}.tmp`,
    windows
      ? `@echo off\r\n"${options.executable}" "${entry}" %*\r\n`
      : `#!/bin/sh\nexec ${shellQuote(options.executable)} ${shellQuote(entry)} "$@"\n`,
    { mode: 0o700 },
  );
  if (!windows) chmodSync(`${launcher}.tmp`, 0o700);
  renameSync(`${launcher}.tmp`, launcher);
  return { executable: options.executable, entry, launcher };
}

/** Removes runtimes other than `keep`, after the new host is running. */
export function pruneRuntimes(directory: string, keep: string): void {
  const runtimes = join(directory, "runtime");
  let entries: string[] = [];
  try {
    entries = readdirSync(runtimes);
  } catch {
    return;
  }
  for (const entry of entries)
    if (resolve(runtimes, entry) !== resolve(keep))
      rmSync(join(runtimes, entry), { recursive: true, force: true });
  // Hosts installed from release archives kept this pointer beside `bin`.
  rmSync(join(directory, "runtime-path"), { force: true });
}

export const bundleDirectory = (entry: string) => dirname(entry);

/** Whether `entry` already runs from `<data dir>/runtime`, as a host
 * installed from a release archive by SSH setup does. */
export function installedRuntime(directory: string, entry: string): boolean {
  const path = relative(join(directory, "runtime"), entry);
  return !!path && !path.startsWith("..") && !isAbsolute(path);
}

/**
 * The Node executable for the background service. Homebrew and similar
 * installs run Node from a versioned folder that an upgrade deletes; a PATH
 * entry that links to the same binary, such as /opt/homebrew/bin/node,
 * survives the upgrade.
 */
export function stableNodePath(
  executable = process.execPath,
  path = process.env.PATH ?? "",
  platform: NodeJS.Platform = process.platform,
): string {
  let real: string;
  try {
    real = realpathSync(executable);
  } catch {
    return executable;
  }
  const separator = platform === "win32" ? ";" : delimiter;
  for (const folder of path.split(separator)) {
    if (!folder) continue;
    const candidate = join(folder, platform === "win32" ? "node.exe" : "node");
    if (resolve(candidate) === resolve(executable)) continue;
    try {
      if (realpathSync(candidate) === real) return candidate;
    } catch {
      /* not in this folder */
    }
  }
  return executable;
}

import { DatabaseSync } from "node:sqlite";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { runPowerShell } from "./windows";

const exec = promisify(execFile);

/** Kernel-backed SQLite locking survives neither crashes nor PID reuse. The
 * PID file remains for compatibility with hosts installed before this lock. */
export async function acquireHostOwner(directory: string): Promise<() => void> {
  const db = new DatabaseSync(join(directory, "owner.db"));
  const path = join(directory, "owner.lock");
  try {
    try {
      db.exec("BEGIN EXCLUSIVE");
    } catch {
      throw new Error("A host already owns this data directory");
    }
    if (existsSync(path)) {
      const contents = readFileSync(path, "utf8");
      // Version 2 owners hold the SQLite lock we just acquired. Their leftover
      // PID file is therefore stale, regardless of which process owns that PID.
      let modern = false;
      try {
        modern = JSON.parse(contents)?.version === 2;
      } catch {
        /* legacy PID file */
      }
      if (!modern) {
        const pid = Number(contents);
        if (!Number.isSafeInteger(pid) || pid < 1)
          throw new Error(
            "Host lock is incomplete; inspect owner.lock before removing it",
          );
        let alive = true;
        try {
          process.kill(pid, 0);
        } catch (error) {
          if ((error as NodeJS.ErrnoException).code === "ESRCH") alive = false;
        }
        if (alive) {
          // Never treat an unresponsive host as dead. For legacy locks, check
          // the process command, conservatively refusing if it cannot be read.
          const command =
            process.platform === "win32"
              ? await runPowerShell(
                  `$p = Get-CimInstance Win32_Process -Filter 'ProcessId = ${pid}'; if ($null -eq $p.CommandLine) { throw 'Cannot inspect host lock owner' }; $p.CommandLine`,
                )
              : (
                  await exec("ps", ["-ww", "-p", String(pid), "-o", "args="], {
                    timeout: 5_000,
                  })
                ).stdout;
          if (!command.trim() || /monocode-host.*\bserve\b/.test(command))
            throw new Error("A host already owns this data directory");
        }
      }
    }
    // Old clients cannot mistake this for a dead PID and take ownership.
    writeFileSync(path, JSON.stringify({ version: 2, pid: process.pid }), {
      mode: 0o600,
    });
  } catch (error) {
    db.close();
    throw error;
  }
  return () => {
    try {
      rmSync(path, { force: true });
    } finally {
      db.close();
    }
  };
}

import { spawn } from "node:child_process";
import { createReadStream } from "node:fs";
import { join } from "node:path";

// fd 3 belongs only to the host. EOF means it exited, even after a hard crash.
const parent = createReadStream("/dev/null", { fd: 3, autoClose: false });
parent.resume();
const [command, ...args] = process.argv.slice(2);
if (!command) throw new Error("Missing provider command");
const child = spawn(command, args, {
  cwd: process.cwd(),
  env: process.env,
  stdio: "pipe",
  detached: process.platform !== "win32",
  windowsHide: true,
});
process.stdin.pipe(child.stdin);
child.stdout.pipe(process.stdout);
child.stderr.pipe(process.stderr);
child.stdin.on("error", () => {});

let stopping = false;
let exitCode = 1;
function finish() {
  parent.destroy();
  process.exitCode = exitCode;
}
function stopTree() {
  if (stopping) return;
  stopping = true;
  if (process.platform === "win32") {
    const killer = spawn(
      join(process.env.SystemRoot ?? "C:\\Windows", "System32", "taskkill.exe"),
      ["/PID", String(child.pid), "/T", "/F"],
      {
        stdio: "ignore",
        windowsHide: true,
      },
    );
    killer.on("error", () => child.kill());
    killer.on("close", finish);
  } else {
    // The provider leads its own process group, so the guard survives long
    // enough to escalate if one of its descendants ignores SIGTERM.
    try {
      process.kill(-child.pid, "SIGTERM");
    } catch {
      finish();
      return;
    }
    setTimeout(() => {
      try {
        process.kill(-child.pid, "SIGKILL");
      } catch {
        /* exited */
      }
      finish();
    }, 1_000);
  }
}
parent.on("end", stopTree);
parent.on("error", stopTree);
process.on("SIGTERM", stopTree);
process.on("SIGINT", stopTree);
child.on("error", (error) => {
  console.error(error.message);
  stopTree();
});
child.on("close", (code) => {
  exitCode = code ?? 1;
  stopTree();
});

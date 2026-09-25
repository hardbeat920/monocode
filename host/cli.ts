import {
  chmodSync,
  closeSync,
  existsSync,
  mkdirSync,
  openSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import {
  configureChildBackend,
  acquireHarnessBridge,
} from "../src/integrations/harness/core/child";
import { HostChildBackend } from "./child-backend";
import { HostStore } from "./store";
import { HostEngine } from "./engine";
import { hostProviders } from "./providers";
import { createHostServer } from "./server";
import type { RemoteProvider } from "../src/features/connections/model/protocol";
import { connectionInfo, installService } from "./service";
import { version } from "../package.json";
import { protectWindowsDirectory } from "./windows";

process.umask(0o077);
// npm-based providers can launch Node subprocesses without a separate Node
// installation. Keep the packaged runtime first in the host's own PATH.
process.env.PATH = [dirname(process.execPath), process.env.PATH ?? ""].join(
  delimiter,
);
const args = process.argv.slice(2);
const command = args[0] ?? "help";
const option = (name: string, fallback: string): string => {
  const i = args.indexOf(`--${name}`);
  if (i < 0) return fallback;
  if (!args[i + 1] || args[i + 1].startsWith("--"))
    throw new Error(`Missing --${name} value`);
  return args[i + 1];
};
const directory = resolve(
  option("data-dir", join(homedir(), ".monocode-host")),
);
const port = Number(option("port", "3774"));
const statePath = join(directory, "running.json");
type Running = { pid: number; port: number; secret: string };
const readRunning = (): Running | undefined => {
  if (!existsSync(statePath)) return;
  return JSON.parse(readFileSync(statePath, "utf8")) as Running;
};

async function main() {
  if (command === "--version") {
    console.log(version);
    return;
  }
  if (command === "help" || command === "--help") {
    console.log(`MonoCode Host (experimental; Node 24+; Windows/Linux/macOS)
  serve                 Run in foreground on 127.0.0.1
  start                 Run detached from this terminal
  service install       Install/start the persistent user service
  connection-info       Print the running host's port (JSON)
  status                Check the running host
  stop                  Stop the host and interrupt its running turns
  pair --name <device>  Issue a device credential (shown once)
  devices               List paired devices
  revoke <device-id>    Revoke a device credential
Options: --data-dir <directory> --port <port> (default 3774)
Connect another computer using an SSH forward to the loopback port.`);
    return;
  }
  if (!Number.isInteger(port) || port < 1 || port > 65535)
    throw new Error("Invalid port");
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  if (process.platform === "win32") await protectWindowsDirectory(directory);
  else chmodSync(directory, 0o700);
  if (command === "connection-info") {
    console.log(JSON.stringify(await connectionInfo(directory)));
    return;
  }
  if (command === "service") {
    if (args[1] !== "install") throw new Error("Use: service install");
    console.log(
      JSON.stringify(
        await installService({
          directory,
          port,
          executable: process.execPath,
          entry: fileURLToPath(import.meta.url),
        }),
      ),
    );
    return;
  }
  if (command === "status" || command === "stop") {
    const state = readRunning();
    if (!state) {
      console.log("Host is stopped");
      return;
    }
    const response = await fetch(`http://127.0.0.1:${state.port}/lifecycle`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${state.secret}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ action: command }),
      signal: AbortSignal.timeout(5_000),
    });
    if (!response.ok) throw new Error("Could not verify the running host");
    console.log(
      command === "stop"
        ? "Host is stopping"
        : `Host is running (PID ${state.pid}, port ${state.port})`,
    );
    return;
  }
  if (command === "start") {
    const log = openSync(join(directory, "host.log"), "a", 0o600);
    const child = spawn(
      process.execPath,
      [
        fileURLToPath(import.meta.url),
        "serve",
        "--data-dir",
        directory,
        "--port",
        String(port),
      ],
      {
        detached: true,
        windowsHide: true,
        stdio: ["ignore", log, log],
        env: process.env,
      },
    );
    child.unref();
    closeSync(log);
    for (let i = 0; i < (process.platform === "win32" ? 150 : 50); i++) {
      await new Promise((resolve) => setTimeout(resolve, 100));
      const state = readRunning();
      if (state && state.pid === child.pid) {
        console.log(
          `Host started on 127.0.0.1:${state.port}. It will continue after this terminal closes.`,
        );
        return;
      }
    }
    throw new Error(`Host did not start. See ${join(directory, "host.log")}`);
  }
  const store = new HostStore(join(directory, "host.db"));
  if (command === "pair") {
    const device = store.issueDevice(option("name", "Desktop"));
    console.log(
      JSON.stringify(
        { ...device, environmentId: store.environmentId },
        null,
        args.includes("--json") ? undefined : 2,
      ),
    );
    store.close();
    return;
  }
  if (command === "devices") {
    console.log(
      JSON.stringify(
        store.db.prepare("SELECT id, name FROM devices").all(),
        null,
        2,
      ),
    );
    store.close();
    return;
  }
  if (command === "revoke") {
    if (!args[1] || args[1].startsWith("--"))
      throw new Error("Provide a device ID to revoke");
    store.db.prepare("DELETE FROM devices WHERE id=?").run(args[1]);
    store.close();
    console.log("Device revoked");
    return;
  }
  if (command !== "serve") {
    store.close();
    throw new Error("Unknown command; run with --help");
  }
  const lock = join(directory, "owner.lock");
  if (existsSync(lock)) {
    const oldPid = Number(readFileSync(lock, "utf8"));
    if (!Number.isSafeInteger(oldPid) || oldPid < 1)
      throw new Error(
        "Host lock is incomplete; inspect the host before removing owner.lock",
      );
    let alive = true;
    try {
      process.kill(oldPid, 0);
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === "ESRCH") alive = false;
    }
    if (alive) throw new Error("A host already owns this data directory");
    rmSync(lock);
  }
  const fd = openSync(lock, "wx", 0o600);
  writeFileSync(fd, String(process.pid));
  closeSync(fd);
  const backend = new HostChildBackend();
  let cleanup = () => {
    rmSync(lock, { force: true });
    rmSync(statePath, { force: true });
    store.close();
  };
  try {
    configureChildBackend(backend);
    const release = await acquireHarnessBridge();
    const available: RemoteProvider[] = [];
    for (const provider of ["codex", "claude"] as const) {
      try {
        await backend.resolve(provider);
        available.push(provider);
      } catch {
        /* report via descriptor */
      }
    }
    const engine = new HostEngine(store, hostProviders);
    const secret = randomBytes(32).toString("base64url");
    let stopping = false;
    let stop: () => Promise<void>;
    // A separate local administrative credential cannot be used as a paired
    // client credential, and is never sent to the desktop.
    const server = createHostServer(engine, available, (request, response) => {
      if (
        request.method !== "POST" ||
        request.headers.origin ||
        request.headers.authorization !== `Bearer ${secret}`
      ) {
        response.writeHead(403).end();
        return;
      }
      let body = "";
      request.on("data", (chunk) => {
        body += String(chunk);
        if (body.length > 128) request.destroy();
      });
      request.on("end", () => {
        try {
          const action = JSON.parse(body).action;
          if (action !== "status" && action !== "stop") {
            response.writeHead(400).end();
            return;
          }
          response.end("{}");
          if (action === "stop") void stop();
        } catch {
          response.writeHead(400).end();
        }
      });
    });
    stop = async () => {
      if (stopping) return;
      stopping = true;
      server.close();
      server.closeAllConnections();
      await engine.close();
      await backend.close();
      release();
      cleanup();
    };
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(port, "127.0.0.1", resolve);
    });
    writeFileSync(
      statePath,
      JSON.stringify({ pid: process.pid, port, secret }),
      { mode: 0o600 },
    );
    process.once("SIGTERM", () => {
      void stop();
    });
    process.once("SIGINT", () => {
      void stop();
    });
    console.log(
      `MonoCode Host ${store.environmentId} listening on 127.0.0.1:${port}`,
    );
    console.log(
      `Providers: ${available.join(", ") || "none found; install and authenticate Codex or Claude on this host"}`,
    );
    cleanup = () => {
      rmSync(lock, { force: true });
      rmSync(statePath, { force: true });
      store.close();
    };
  } catch (error) {
    await backend.close();
    cleanup();
    throw error;
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});

import { chmodSync, existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { randomBytes } from "node:crypto";
import type { Server as NetServer } from "node:net";
import {
  configureChildBackend,
  acquireHarnessBridge,
} from "../src/integrations/harness/core/child";
import { HostChildBackend } from "./child-backend";
import { HostStore } from "./store";
import { acquireHostOwner } from "./owner";
import { HostEngine } from "./engine";
import { hostProviders } from "./providers";
import { createHostServer } from "./server";
import {
  REMOTE_PROVIDERS,
  type RemoteProvider,
} from "../src/features/connections/model/protocol";
import { connectionInfo, installService, uninstallService } from "./service";
import { version } from "../package.json";
import { protectWindowsDirectory } from "./windows";
import { lifecycle, readRunning, type HostStatus } from "./control";
import {
  connect,
  connectDisable,
  connectPair,
  connectStatus,
  startDetached,
} from "./connect";
import { listenHost } from "./listener";
import {
  networkEndpoints,
  readNetworkSettings,
  tailscaleName,
  type NetworkSettings,
} from "./network";
import { loadHostIdentity } from "./tls";

process.umask(0o077);
// npm-based providers can launch Node subprocesses without a separate Node
// installation. Keep the host's own Node first in its PATH.
process.env.PATH = [dirname(process.execPath), process.env.PATH ?? ""].join(
  delimiter,
);
const args = process.argv.slice(2);
const command = args[0] ?? "help";
const flag = (name: string) => args.includes(`--${name}`);
const option = (name: string): string | undefined => {
  const i = args.indexOf(`--${name}`);
  if (i < 0) return undefined;
  if (!args[i + 1] || args[i + 1].startsWith("--"))
    throw new Error(`Missing --${name} value`);
  return args[i + 1];
};
const directory = resolve(option("data-dir") ?? join(homedir(), ".monocode-host"));
const requestedPort = option("port") === undefined ? undefined : Number(option("port"));
const port = requestedPort ?? 3774;
const statePath = join(directory, "running.json");
const entry = fileURLToPath(import.meta.url);

const HELP = `MonoCode Host ${version}

Set up this machine for MonoCode:
  npx monocode-host connect           Install the host as a background service, turn on
                                      network access, and print a pairing link
    --local-only                      Listen on loopback only; pair over SSH
    --bind <address>                  Listen on one address instead of all (0.0.0.0)
    --name <label>                    Name shown in MonoCode (default: hostname)
    --no-service                      Run detached instead of as a login service
    --restart                         Reinstall and restart this version
    --yes                             Restart without asking, interrupting running turns
    --json                            Print one JSON line; progress goes to stderr
  connect pair [--json]               Print a new one-time pairing link
  connect status [--json]             Show the host, network access, and paired desktops
  connect disable                     Turn off network access; SSH pairing keeps working

Manage the host:
  status | stop | start | serve       Check, stop, start detached, or run in the foreground
  service install | uninstall         Add or remove the login service; data is kept
  devices                             List paired desktops
  revoke <device-id>                  Revoke a desktop's access
  connection-info                     Print the running host's port (JSON)
  pair --name <device>                Issue a raw device credential (advanced)

Options: --data-dir <directory> (default ~/.monocode-host) --port <port> (default 3774)
Requires Node.js 22.13 or newer.`;

async function main() {
  if (command === "--version" || command === "-v") {
    console.log(version);
    return;
  }
  if (command === "help" || command === "--help" || command === "-h") {
    console.log(HELP);
    return;
  }
  if (!Number.isInteger(port) || port < 1 || port > 65535)
    throw new Error("Invalid port");
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  if (process.platform === "win32") await protectWindowsDirectory(directory);
  else chmodSync(directory, 0o700);

  if (command === "connect") {
    const sub = args[1] && !args[1].startsWith("--") ? args[1] : undefined;
    if (sub === "pair")
      return connectPair({ directory, json: flag("json"), name: option("name") });
    if (sub === "status") return connectStatus({ directory, json: flag("json") });
    if (sub === "disable") return connectDisable(directory);
    if (sub) throw new Error(`Unknown connect command: ${sub}. Run with --help.`);
    return connect({
      directory,
      port: requestedPort,
      version,
      entry,
      bind: option("bind"),
      localOnly: flag("local-only"),
      json: flag("json"),
      yes: flag("yes"),
      service: !flag("no-service"),
      restart: flag("restart"),
      name: option("name"),
    });
  }
  if (command === "connection-info") {
    console.log(JSON.stringify(await connectionInfo(directory)));
    return;
  }
  if (command === "service" && args[1] === "uninstall") {
    const notes = await uninstallService();
    // Also stops a manually started host, or one the service manager left.
    const state = readRunning(directory);
    if (state) {
      await lifecycle(state, "stop").catch(() => undefined);
      for (let i = 0; i < 200 && existsSync(statePath); i++)
        await new Promise((resolve) => setTimeout(resolve, 100));
    }
    console.log(
      [
        existsSync(statePath)
          ? "The host service was removed, but the host is still running. Run stop, or end its process."
          : "The host is stopped and will not start automatically.",
        `Sessions, logs and device credentials are kept in ${directory}. Delete that directory only if you want to erase them.`,
        ...notes,
      ].join("\n"),
    );
    return;
  }
  if (command === "service") {
    if (args[1] !== "install")
      throw new Error("Use: service install, or service uninstall");
    console.log(
      JSON.stringify(
        await installService({
          directory,
          port,
          executable: process.execPath,
          entry,
        }),
      ),
    );
    return;
  }
  if (command === "status" || command === "stop") {
    const state = readRunning(directory);
    if (!state) {
      console.log("Host is stopped");
      return;
    }
    const status = await lifecycle(state, command);
    console.log(
      command === "stop"
        ? "Host is stopping"
        : `Host${status.version ? ` ${status.version}` : ""} is running (PID ${state.pid}, port ${state.port})`,
    );
    return;
  }
  if (command === "start") {
    const state = await startDetached({
      directory,
      port,
      executable: process.execPath,
      entry,
    });
    console.log(
      `Host started on port ${state.port}. It will continue after this terminal closes.`,
    );
    return;
  }
  const store = new HostStore(join(directory, "host.db"));
  if (command === "pair") {
    const device = store.issueDevice(option("name") ?? "Desktop");
    console.log(
      JSON.stringify(
        { ...device, environmentId: store.environmentId },
        null,
        flag("json") ? undefined : 2,
      ),
    );
    store.close();
    return;
  }
  if (command === "devices") {
    console.log(JSON.stringify(store.devices(), null, 2));
    store.close();
    return;
  }
  if (command === "revoke") {
    if (!args[1] || args[1].startsWith("--"))
      throw new Error("Provide a device ID to revoke");
    const revoked = store.revokeDevice(args[1]);
    store.close();
    if (!revoked) throw new Error("Device not found");
    console.log("Device revoked");
    return;
  }
  if (command !== "serve") {
    store.close();
    throw new Error("Unknown command; run with --help");
  }
  await serve(store);
}

async function serve(store: HostStore) {
  const releaseOwner = await acquireHostOwner(directory);
  const backend = new HostChildBackend();
  let cleanup = () => {
    rmSync(statePath, { force: true });
    store.close();
    releaseOwner();
  };
  try {
    configureChildBackend(backend);
    const release = await acquireHarnessBridge();
    const available: RemoteProvider[] = [];
    for (const provider of REMOTE_PROVIDERS) {
      try {
        await backend.resolve(provider);
        available.push(provider);
      } catch {
        /* report via descriptor */
      }
    }
    const engine = new HostEngine(store, hostProviders);
    const identity = loadHostIdentity(directory);
    const secret = randomBytes(32).toString("base64url");
    let network: NetworkSettings & { error?: string } = readNetworkSettings(directory);
    let magicDns: string | undefined;
    void tailscaleName().then((name) => (magicDns = name));
    let front: NetServer | undefined;
    let stopping = false;
    let stop: () => Promise<void>;
    const status = (): HostStatus => ({
      version,
      pid: process.pid,
      port,
      runningTurns: store.runningTurns(),
      providers: available,
      network,
    });
    // A separate local administrative credential cannot be used as a paired
    // client credential, and is never sent to the desktop.
    const server = createHostServer(
      engine,
      available,
      (request, response) => {
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
        request.on("end", async () => {
          let action: unknown;
          try {
            action = JSON.parse(body).action;
          } catch {
            response.writeHead(400).end();
            return;
          }
          if (action === "network") await applyNetwork();
          else if (action !== "status" && action !== "stop") {
            response.writeHead(400).end();
            return;
          }
          response.setHeader("Content-Type", "application/json");
          response.end(JSON.stringify(status()));
          if (action === "stop") void stop();
        });
      },
      {
        endpoints: () =>
          network.enabled && !network.error
            ? networkEndpoints(port, network.bind, undefined, magicDns ? [magicDns] : [])
            : [],
      },
    );
    const open = (settings: NetworkSettings) =>
      listenHost(server, {
        port,
        bind: settings.enabled ? settings.bind : "127.0.0.1",
        identity,
      });
    // Listening on a network address failed, such as when the address no
    // longer exists. Keep loopback working and report why.
    const openOrLoopback = async (settings: NetworkSettings) => {
      try {
        front = await open(settings);
        network = settings;
      } catch (error) {
        if (!settings.enabled) throw error;
        front = await open({ ...settings, enabled: false });
        network = {
          ...settings,
          error: error instanceof Error ? error.message : String(error),
        };
      }
    };
    // Rebinds without a restart, so turning network access on or off does
    // not interrupt agents. Open connections are unaffected.
    const applyNetwork = async () => {
      const next = readNetworkSettings(directory);
      const bind = (settings: NetworkSettings) =>
        settings.enabled ? settings.bind : "127.0.0.1";
      if (!network.error && bind(next) === bind(network)) {
        network = next;
        return;
      }
      front?.close();
      for (let attempt = 0; ; attempt++) {
        try {
          await openOrLoopback(next);
          return;
        } catch (error) {
          if (attempt >= 20) throw error;
          await new Promise((resolve) => setTimeout(resolve, 100));
        }
      }
    };
    stop = async () => {
      if (stopping) return;
      stopping = true;
      front?.close();
      server.close();
      server.closeAllConnections();
      store.changes.close();
      await engine.close();
      await backend.close();
      release();
      cleanup();
    };
    await openOrLoopback(network);
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
      `MonoCode Host ${version} (${store.environmentId}) listening on ${network.enabled && !network.error ? `${network.bind}:${port} (TLS; plain HTTP from loopback)` : `127.0.0.1:${port}`}`,
    );
    if (network.error)
      console.log(`Network access failed, serving loopback only: ${network.error}`);
    console.log(
      `Providers: ${available.join(", ") || "none found; install and authenticate a supported provider on this host"}`,
    );
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

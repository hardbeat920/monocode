import { closeSync, existsSync, openSync } from "node:fs";
import { spawn } from "node:child_process";
import { hostname } from "node:os";
import { dirname, join } from "node:path";
import { createInterface } from "node:readline/promises";
import {
  compareVersions,
  lifecycle,
  readRunning,
  runningStatus,
  type HostStatus,
  type RunningHost,
} from "./control";
import {
  DEFAULT_BIND,
  networkEndpoints,
  pairingLink,
  readNetworkSettings,
  tailscaleName,
  writeNetworkSettings,
} from "./network";
import { installRuntime, pruneRuntimes, stableNodePath } from "./runtime";
import { installService, uninstallService } from "./service";
import { HostStore } from "./store";
import { loadHostIdentity } from "./tls";

export type ConnectOptions = {
  directory: string;
  /** Defaults to the running host's port, then 3774. */
  port?: number;
  version: string;
  /** The running bundle, such as the copy in npx's cache. */
  entry: string;
  bind?: string;
  localOnly: boolean;
  json: boolean;
  /** Restart a host that has running turns without asking. */
  yes: boolean;
  /** Install a login service. Without it, the host runs detached. */
  service: boolean;
  /** Reinstall and restart even when this version is already running. */
  restart: boolean;
  name?: string;
};

type Output = { say: (line?: string) => void; json: boolean };
const output = (json: boolean): Output => ({
  json,
  // Progress goes to stderr in JSON mode, so stdout carries one JSON line.
  say: (line = "") => (json ? process.stderr : process.stdout).write(`${line}\n`),
});

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** Starts `serve` detached from this terminal and waits for it to answer. */
export async function startDetached(options: {
  directory: string;
  port: number;
  executable: string;
  entry: string;
}): Promise<RunningHost> {
  const log = openSync(join(options.directory, "host.log"), "a", 0o600);
  const child = spawn(
    options.executable,
    [
      options.entry,
      "serve",
      "--data-dir",
      options.directory,
      "--port",
      String(options.port),
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
    await sleep(100);
    const state = readRunning(options.directory);
    if (state && state.pid === child.pid) return state;
  }
  throw new Error(
    `Host did not start. See ${join(options.directory, "host.log")}`,
  );
}

/** Stops the running host. With `service`, first removes the login service
 * so its manager does not start the old version again. */
export async function stopHost(
  directory: string,
  state: RunningHost | undefined,
  service: boolean,
): Promise<void> {
  if (service) await uninstallService().catch(() => []);
  if (state) await lifecycle(state, "stop").catch(() => undefined);
  const running = join(directory, "running.json");
  for (let i = 0; i < 200 && existsSync(running); i++) await sleep(100);
  if (existsSync(running) && (await runningStatus(directory)))
    throw new Error(
      "The running host did not stop. Stop it with `monocode-host stop`, then run connect again.",
    );
}

async function confirm(question: string): Promise<boolean> {
  if (!process.stdin.isTTY) return false;
  const prompt = createInterface({ input: process.stdin, output: process.stdout });
  try {
    return /^y(es)?$/i.test((await prompt.question(`${question} [y/N] `)).trim());
  } finally {
    prompt.close();
  }
}

const serviceDescription = (kind: string) =>
  ({
    linux: "Background service running (systemd user service; keeps running after logout)",
    darwin: "Background service running (launch agent; runs while you are logged in to this Mac)",
    win32: "Background service running (Task Scheduler; runs while you are signed in to Windows)",
    detached: "Host running in the background until this machine restarts or you log out",
  })[kind] ?? "Host running";

const PROVIDER_NAMES: Record<string, string> = {
  codex: "Codex",
  claude: "Claude Code",
};

/** Endpoints for the network settings a host reports. */
async function advertised(port: number, network?: HostStatus["network"]) {
  if (!network?.enabled) return [];
  const name = await tailscaleName();
  return networkEndpoints(port, network.bind, undefined, name ? [name] : []);
}

function issueLink(options: {
  directory: string;
  endpoints: string[];
  name?: string;
}) {
  const identity = loadHostIdentity(options.directory);
  const store = new HostStore(join(options.directory, "host.db"));
  try {
    const pairing = store.issuePairing();
    return {
      environmentId: store.environmentId,
      fingerprint: identity.fingerprint,
      expiresAt: pairing.expiresAt,
      link: pairingLink({
        name: options.name?.trim().slice(0, 100) || hostname(),
        environmentId: store.environmentId,
        fingerprint: identity.fingerprint,
        code: pairing.code,
        endpoints: options.endpoints,
      }),
    };
  } finally {
    store.close();
  }
}

function printLink(out: Output, link: string, endpoints: string[]) {
  out.say();
  out.say("Pair a desktop");
  out.say(
    "  In MonoCode, open Settings → Connections → Pair machine and paste this link.",
  );
  out.say("  It works once and expires in 15 minutes.");
  out.say();
  out.say(`  ${link}`);
  if (!endpoints.length) {
    out.say();
    out.say(
      "  This host listens only on loopback. Pair it from MonoCode with Set up over SSH,",
    );
    out.say("  or run connect without --local-only to enable network access.");
  }
}

/**
 * `monocode-host connect`: installs this version as a background service,
 * enables network access over TLS, and prints a one-time pairing link.
 * Running it again reuses a running host of the same or a newer version, so it
 * is also how a new desktop gets a link.
 */
export async function connect(options: ConnectOptions): Promise<void> {
  const out = output(options.json);
  const { directory } = options;
  out.say("MonoCode Connect");
  out.say();

  const previous = readNetworkSettings(directory);
  const network = {
    enabled: !options.localOnly,
    bind: options.bind ?? previous.bind ?? DEFAULT_BIND,
  };
  writeNetworkSettings(directory, network);
  loadHostIdentity(directory);

  let status = await runningStatus(directory);
  let service = "existing";
  const reuse =
    status?.version &&
    !options.restart &&
    compareVersions(status.version, options.version) >= 0;
  if (status && reuse) {
    const applied = await lifecycle(status.state, "network");
    status = { ...status, ...applied };
    out.say(`✓ Host ${status.version} is running (PID ${status.state.pid})`);
    if (compareVersions(status.version!, options.version) > 0)
      out.say(
        `  It is newer than this command (${options.version}), so it was left unchanged.`,
      );
  } else {
    if (status) {
      // Hosts before 0.5 do not report their turns, so assume some may run.
      const turns = status.runningTurns;
      if ((turns === undefined || turns > 0) && !options.yes) {
        const question =
          turns === undefined
            ? "An older host is running. Updating restarts it and interrupts any running turns. Continue?"
            : `The host has ${turns} running turn${turns === 1 ? "" : "s"}. Updating restarts it and interrupts them. Continue?`;
        if (options.json || !(await confirm(question)))
          throw new Error(
            options.json || !process.stdin.isTTY
              ? `${question.replace(/ Continue\?$/, "")} Run connect again with --yes to update anyway.`
              : "Update cancelled. The running host was left unchanged.",
          );
      }
      out.say(
        `Updating the host from ${status.version ?? "an older version"} to ${options.version}…`,
      );
      await stopHost(directory, status.state, options.service);
    } else if (options.service) {
      // A stale service definition may point at a removed runtime.
      await uninstallService().catch(() => []);
    }
    const runtime = installRuntime({
      directory,
      version: options.version,
      source: dirname(options.entry),
      executable: stableNodePath(),
    });
    const target = {
      directory,
      port: options.port ?? status?.state.port ?? 3774,
      executable: runtime.executable,
      entry: runtime.entry,
    };
    if (options.service) {
      try {
        await installService(target);
        service = process.platform;
      } catch (error) {
        out.say(
          `! The background service could not be installed: ${error instanceof Error ? error.message : String(error)}`,
        );
        // Remove what was installed and wait for any host it started to
        // exit, so two hosts never share the data directory. This throws
        // instead of starting a second host if one is still running.
        await stopHost(directory, readRunning(directory), true);
        await startDetached(target);
        service = "detached";
      }
    } else {
      await startDetached(target);
      service = "detached";
    }
    status = await runningStatus(directory);
    if (!status)
      throw new Error(
        `The host did not answer after starting. See ${join(directory, "host.log")}`,
      );
    pruneRuntimes(directory, dirname(runtime.entry));
    out.say(`✓ Host ${options.version} installed in ${directory}`);
    out.say(`✓ ${serviceDescription(service)}`);
    out.say(`  Manage it with ${runtime.launcher}`);
  }

  const providers = (status.providers ?? []).map((id) => PROVIDER_NAMES[id] ?? id);
  out.say(
    providers.length
      ? `✓ Providers: ${providers.join(", ")}`
      : "! No Codex or Claude Code CLI was found. Install one and sign in as this user, then run connect --restart.",
  );

  const port = status.state.port;
  const endpoints = await advertised(port, status.network);
  if (status.network?.error)
    out.say(`! Network access is off: ${status.network.error}`);
  else if (status.network?.enabled) {
    out.say(`✓ Network access on port ${port} (TLS)`);
    for (const endpoint of endpoints) out.say(`    ${endpoint}`);
    if (!endpoints.length)
      out.say("  No network address was found. Pair over SSH instead.");
  } else out.say(`✓ Loopback only, on 127.0.0.1:${port}`);

  const issued = issueLink({ directory, endpoints, name: options.name });
  if (options.json) {
    process.stdout.write(
      `${JSON.stringify({
        link: issued.link,
        version: status.version,
        port,
        environmentId: issued.environmentId,
        fingerprint: issued.fingerprint,
        endpoints,
        expiresAt: issued.expiresAt,
        service,
      })}\n`,
    );
    return;
  }
  printLink(out, issued.link, endpoints);
  out.say();
  out.say("Run `monocode-host connect pair` for another link.");
}

/** `monocode-host connect pair`: a new link for the running host. */
export async function connectPair(options: {
  directory: string;
  json: boolean;
  name?: string;
}): Promise<void> {
  const out = output(options.json);
  const status = await runningStatus(options.directory);
  if (!status)
    throw new Error(
      "MonoCode Host is not running. Run `npx monocode-host connect` to install and start it.",
    );
  const endpoints = await advertised(status.state.port, status.network);
  const issued = issueLink({ ...options, endpoints });
  if (options.json) {
    process.stdout.write(
      `${JSON.stringify({ ...issued, endpoints, port: status.state.port, version: status.version })}\n`,
    );
    return;
  }
  printLink(out, issued.link, endpoints);
}

/** `monocode-host connect status`. */
export async function connectStatus(options: {
  directory: string;
  json: boolean;
}): Promise<void> {
  const status = await runningStatus(options.directory);
  const settings = readNetworkSettings(options.directory);
  const identity = loadHostIdentity(options.directory);
  const store = new HostStore(join(options.directory, "host.db"));
  let devices: { id: string; name: string }[];
  let pending: number;
  let environmentId: string;
  try {
    devices = store.devices();
    pending = store.pendingPairings();
    environmentId = store.environmentId;
  } finally {
    store.close();
  }
  const network = status?.network ?? settings;
  const endpoints = status ? await advertised(status.state.port, network) : [];
  const report = {
    running: !!status,
    version: status?.version,
    pid: status?.state.pid,
    port: status?.state.port,
    providers: status?.providers ?? [],
    runningTurns: status?.runningTurns ?? 0,
    network,
    endpoints,
    fingerprint: identity.fingerprint,
    environmentId,
    devices,
    pendingPairings: pending,
  };
  if (options.json) {
    console.log(JSON.stringify(report, null, 2));
    return;
  }
  console.log("MonoCode Connect");
  console.log();
  console.log(
    status
      ? `  Host: ${status.version ?? "unknown version"}, PID ${status.state.pid}, port ${status.state.port}`
      : "  Host: stopped. Run `npx monocode-host connect` to start it.",
  );
  if (status)
    console.log(
      `  Providers: ${report.providers.map((id) => PROVIDER_NAMES[id] ?? id).join(", ") || "none found"}`,
    );
  console.log(
    `  Network: ${network.enabled ? `on (${network.bind}, TLS)` : "loopback only"}${status?.network?.error ? `, failed: ${status.network.error}` : ""}`,
  );
  for (const endpoint of endpoints) console.log(`    ${endpoint}`);
  console.log(`  Certificate: ${identity.fingerprint}`);
  console.log(`  Paired desktops: ${devices.length}`);
  for (const device of devices) console.log(`    ${device.name} (${device.id})`);
  if (pending) console.log(`  Unused pairing links: ${pending}`);
}

/** `monocode-host connect disable`: loopback only; keeps paired desktops. */
export async function connectDisable(directory: string): Promise<void> {
  const settings = readNetworkSettings(directory);
  writeNetworkSettings(directory, { ...settings, enabled: false });
  const state = readRunning(directory);
  if (state) await lifecycle(state, "network").catch(() => undefined);
  console.log(
    "Network access is off. The host listens only on loopback; paired desktops can still reach it through SSH. Run connect again to turn network access back on.",
  );
}

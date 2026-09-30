import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { homedir, userInfo } from "node:os";
import { join } from "node:path";
import { lifecycle } from "./control";
import {
  runPowerShell,
  windowsTaskScript,
  windowsUninstallScript,
} from "./windows";

const exec = promisify(execFile);
const LABEL = "com.monocode.host";
type ServiceOptions = {
  directory: string;
  port: number;
  executable: string;
  entry: string;
};

export async function connectionInfo(
  directory: string,
): Promise<{ port: number; pid: number }> {
  const state = JSON.parse(
    await readFile(join(directory, "running.json"), "utf8"),
  );
  if (
    !Number.isInteger(state.port) ||
    state.port < 1 ||
    state.port > 65535 ||
    typeof state.secret !== "string" ||
    !/^[\w-]{43}$/.test(state.secret)
  ) {
    throw new Error("Invalid host state");
  }
  await lifecycle(state, "status").catch(() => {
    throw new Error("Host is not ready");
  });
  return { port: state.port, pid: state.pid };
}

const xml = (value: string) =>
  value.replace(
    /[<>&"']/g,
    (char) =>
      ({
        "<": "&lt;",
        ">": "&gt;",
        "&": "&amp;",
        '"': "&quot;",
        "'": "&apos;",
      })[char]!,
  );

export function launchAgent(options: ServiceOptions, path: string): string {
  const args = [
    options.executable,
    options.entry,
    "serve",
    "--data-dir",
    options.directory,
    "--port",
    String(options.port),
  ];
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>${LABEL}</string>
<key>ProgramArguments</key><array>${args.map((arg) => `<string>${xml(arg)}</string>`).join("")}</array>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
<key>ThrottleInterval</key><integer>10</integer>
<key>EnvironmentVariables</key><dict><key>PATH</key><string>${xml(path)}</string></dict>
<key>StandardOutPath</key><string>${xml(join(options.directory, "host.log"))}</string>
<key>StandardErrorPath</key><string>${xml(join(options.directory, "host.log"))}</string>
</dict></plist>\n`;
}

// systemd expands % specifiers in both settings; $ variables only in ExecStart.
const unitQuote = (value: string) =>
  `"${value.replaceAll("\\", "\\\\").replaceAll('"', '\\"').replaceAll("%", "%%").replaceAll("\n", "\\n")}"`;
export function systemdUnit(options: ServiceOptions, path: string): string {
  return `[Unit]
Description=MonoCode Host
After=network.target

[Service]
ExecStart=${[options.executable, options.entry, "serve", "--data-dir", options.directory, "--port", String(options.port)].map((value) => unitQuote(value.replaceAll("$", () => "$$"))).join(" ")}
Environment=${unitQuote(`PATH=${path}`)}
Restart=on-failure
RestartSec=5
UMask=0077
KillMode=control-group
TimeoutStopSec=20

[Install]
WantedBy=default.target
`;
}

// SSH sessions may lack the user bus variables systemctl --user needs.
const systemdEnvironment = (uid: number): NodeJS.ProcessEnv => {
  const runtime = process.env.XDG_RUNTIME_DIR ?? `/run/user/${uid}`;
  return {
    ...process.env,
    XDG_RUNTIME_DIR: runtime,
    DBUS_SESSION_BUS_ADDRESS:
      process.env.DBUS_SESSION_BUS_ADDRESS ?? `unix:path=${runtime}/bus`,
  };
};

type Run = (
  command: string,
  args: string[],
  env?: NodeJS.ProcessEnv,
) => Promise<unknown>;

/**
 * Removes the login service or scheduled task so the host no longer starts
 * automatically, stopping it where the service manager owns the process.
 * Never deletes the data directory: sessions, logs and device credentials stay
 * until the user removes them explicitly. Returns follow-up notes.
 */
export async function uninstallService(
  system: {
    platform?: NodeJS.Platform;
    home?: string;
    run?: Run;
    powershell?: (script: string) => Promise<unknown>;
  } = {},
): Promise<string[]> {
  const platform = system.platform ?? process.platform;
  const home = system.home ?? homedir();
  const run: Run =
    system.run ??
    ((command, args, env = process.env) =>
      exec(command, args, { env, timeout: 30_000, maxBuffer: 128 * 1024 }));
  const ignore = () => undefined;
  if (platform === "darwin") {
    await run("launchctl", [
      "bootout",
      `gui/${process.getuid!()}/${LABEL}`,
    ]).catch(ignore);
    await rm(join(home, "Library/LaunchAgents", `${LABEL}.plist`), {
      force: true,
    });
    return [];
  }
  if (platform === "linux") {
    const user = userInfo();
    const env = systemdEnvironment(user.uid);
    await run(
      "systemctl",
      ["--user", "disable", "--now", "monocode-host.service"],
      env,
    ).catch(ignore);
    await rm(join(home, ".config/systemd/user/monocode-host.service"), {
      force: true,
    });
    await run("systemctl", ["--user", "daemon-reload"], env).catch(ignore);
    return [
      `Lingering is still enabled for ${user.username}; other user services may rely on it. To turn it off: loginctl disable-linger ${user.username}`,
    ];
  }
  if (platform === "win32") {
    await (system.powershell ?? runPowerShell)(windowsUninstallScript());
    return [];
  }
  throw new Error("MonoCode Host supports Windows, Linux and macOS");
}

export async function installService(
  options: ServiceOptions,
): Promise<{ port: number; pid: number }> {
  // Never replace a running host: connecting must not interrupt agent turns.
  try {
    return await connectionInfo(options.directory);
  } catch {
    /* install/start */
  }
  const path = [
    ...new Set([
      process.env.PATH ?? "",
      join(homedir(), ".local/bin"),
      "/opt/homebrew/bin",
      "/usr/local/bin",
      "/usr/bin",
      "/bin",
    ]),
  ].join(":");
  const run = async (command: string, args: string[], env = process.env) =>
    exec(command, args, { env, timeout: 15_000, maxBuffer: 128 * 1024 });
  if (process.platform === "darwin") {
    const domain = `gui/${process.getuid!()}`;
    try {
      await run("launchctl", ["print", domain]);
    } catch {
      throw new Error(
        "Sign in at the Mac's desktop once, then reconnect. MonoCode Host runs as a login service; keep the Mac signed in and awake.",
      );
    }
    const folder = join(homedir(), "Library/LaunchAgents");
    const file = join(folder, `${LABEL}.plist`);
    await mkdir(folder, { recursive: true });
    let loaded = false;
    try {
      await run("launchctl", ["print", `${domain}/${LABEL}`]);
      loaded = true;
    } catch {
      /* first install */
    }
    if (loaded) {
      // A loaded service already has an owner and executable. Start it without
      // rewriting its configuration or sending a kill/restart command.
      await run("launchctl", ["kickstart", `${domain}/${LABEL}`]);
    } else {
      await writeFile(file, launchAgent(options, path), { mode: 0o600 });
      // Right after `bootout`, as when connect replaces an older host,
      // launchd can refuse the same label until it finishes removing it.
      for (let attempt = 1; ; attempt++) {
        try {
          await run("launchctl", ["bootstrap", domain, file]);
          break;
        } catch (error) {
          if (attempt >= 10) throw error;
          await new Promise((resolve) => setTimeout(resolve, 500));
        }
      }
    }
  } else if (process.platform === "linux") {
    const user = userInfo();
    const env = systemdEnvironment(user.uid);
    try {
      await run(
        "loginctl",
        ["enable-linger", user.username, "--no-ask-password"],
        env,
      );
      const linger = await run(
        "loginctl",
        ["show-user", user.username, "--property=Linger", "--value"],
        env,
      );
      if (linger.stdout.trim() !== "yes") throw new Error("linger disabled");
    } catch {
      throw new Error(
        `This host needs systemd user services and lingering to keep sessions running after SSH disconnects. An administrator can enable it with: sudo loginctl enable-linger ${user.username}`,
      );
    }
    const folder = join(homedir(), ".config/systemd/user");
    await mkdir(folder, { recursive: true });
    const file = join(folder, "monocode-host.service");
    try {
      await readFile(file);
    } catch {
      await writeFile(file, systemdUnit(options, path), { mode: 0o600 });
    }
    await run("systemctl", ["--user", "daemon-reload"], env);
    await run(
      "systemctl",
      ["--user", "enable", "--now", "monocode-host.service"],
      env,
    );
  } else if (process.platform === "win32") {
    await runPowerShell(windowsTaskScript(options, process.env.PATH ?? ""));
  } else {
    throw new Error("MonoCode Host supports Windows, Linux and macOS");
  }
  // launchd waits up to its 10 second ThrottleInterval before starting a job
  // whose previous instance just exited, as when connect replaces a host.
  for (let attempt = 0; attempt < 150; attempt++) {
    try {
      return await connectionInfo(options.directory);
    } catch {
      /* starting */
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(
    process.platform === "win32"
      ? `The host task did not start. Sign in to the Windows desktop as the SSH user and keep that account signed in (locking is fine), then reconnect. Check Task Scheduler and ${join(options.directory, "host.log")}.`
      : `The host service was installed but did not start. Check ${join(options.directory, "host.log")}.`,
  );
}

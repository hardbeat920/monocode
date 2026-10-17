import { afterEach, expect, it, vi } from "vitest";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  installService,
  launchAgent,
  systemdUnit,
  uninstallService,
} from "./service";

// Run the real service registration logic against a temporary filesystem and a
// simulated service manager. No user services or real host processes are used.
const system = vi.hoisted(() => ({
  home: "",
  running: true,
  file: "",
  loaded: "",
  calls: [] as string[][],
}));
vi.mock("node:os", async (original) => ({
  ...(await original<typeof import("node:os")>()),
  homedir: () => system.home,
}));
vi.mock("node:child_process", () => ({
  execFile: (
    command: string,
    args: string[],
    _options: unknown,
    callback: (error: Error | null, result?: { stdout: string }) => void,
  ) => {
    system.calls.push([command, ...args]);
    if (args.includes("disable") || args.includes("bootout"))
      system.running = false;
    if (
      command === "launchctl" &&
      args[0] === "print" &&
      args[1].includes("com.monocode.host")
    )
      return callback(new Error("Not loaded"));
    if (args.includes("enable") || args.includes("bootstrap")) {
      system.loaded = readFileSync(system.file, "utf8");
      system.running = true;
    }
    callback(null, { stdout: args.includes("show-user") ? "yes\n" : "" });
  },
}));

afterEach(() => {
  vi.unstubAllGlobals();
  if (system.home) rmSync(system.home, { recursive: true, force: true });
});

it.skipIf(process.platform === "win32").each(["linux", "darwin"] as const)(
  "uninstall/install refreshes the %s service to the new runtime while an ordinary install preserves it",
  async (platform) => {
    system.home = mkdtempSync(join(tmpdir(), "monocode-service-upgrade-"));
    system.calls = [];
    system.running = true;
    system.loaded = "";
    vi.stubGlobal("process", { ...process, platform });
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({ ok: system.running })),
    );
    const folder = join(
      system.home,
      platform === "linux" ? ".config/systemd/user" : "Library/LaunchAgents",
    );
    mkdirSync(folder, { recursive: true });
    system.file = join(
      folder,
      platform === "linux"
        ? "monocode-host.service"
        : "com.monocode.host.plist",
    );
    const directory = join(system.home, ".monocode-host");
    mkdirSync(directory);
    writeFileSync(
      join(directory, "running.json"),
      JSON.stringify({ port: 4774, pid: 123, secret: "a".repeat(43) }),
    );
    writeFileSync(
      join(directory, "host.db"),
      "sessions and device credentials",
    );
    const oldOptions = {
      directory,
      port: 4774,
      executable: "/runtime/old/node",
      entry: "/runtime/old/host.mjs",
    };
    const nextOptions = {
      ...oldOptions,
      executable: "/runtime/new/node",
      entry: "/runtime/new/host.mjs",
    };
    const registration = platform === "linux" ? systemdUnit : launchAgent;
    writeFileSync(system.file, registration(oldOptions, "/usr/bin"));

    await installService(nextOptions);
    expect(readFileSync(system.file, "utf8")).toContain("/runtime/old/");
    expect(system.calls).toEqual([]);

    await uninstallService();
    expect(system.running).toBe(false);
    await installService(nextOptions);
    expect(system.loaded).toContain("/runtime/new/node");
    expect(system.loaded).toContain("/runtime/new/host.mjs");
    expect(system.loaded).toContain("4774");
    expect(system.loaded).not.toContain("/runtime/old/");
    expect(readFileSync(join(directory, "host.db"), "utf8")).toBe(
      "sessions and device credentials",
    );
    if (platform === "linux") {
      expect(system.calls).toContainEqual([
        "systemctl",
        "--user",
        "disable",
        "--now",
        "monocode-host.service",
      ]);
      expect(system.calls).toContainEqual([
        "systemctl",
        "--user",
        "daemon-reload",
      ]);
      expect(system.calls.at(-1)).toEqual([
        "systemctl",
        "--user",
        "enable",
        "--now",
        "monocode-host.service",
      ]);
    }
  },
);

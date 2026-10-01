import { expect, it } from "vitest";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { launchAgent, systemdUnit, uninstallService } from "./service";

it("keeps paths and environment content from injecting service configuration", () => {
  const options = {
    directory: "/Users/a & b/%folder",
    port: 3774,
    executable: '/runtime/a"b/node',
    entry: "/runtime/$name/host.mjs",
  };
  const plist = launchAgent(options, "/bin:<test>&other");
  expect(plist).toContain("a&quot;b/node");
  expect(plist).toContain("/bin:&lt;test&gt;&amp;other");
  expect(plist).not.toContain("<test>");
  const unit = systemdUnit(options, "/bin:/a\n[Service]\nExecStart=/bad");
  expect(unit.match(/^ExecStart=/gm)).toHaveLength(1);
  expect(unit).toContain("%%folder");
  expect(unit).toContain("$$name");
  expect(unit).toContain("KillMode=control-group");
});

it.skipIf(process.platform === "win32").each(["darwin", "linux"] as const)(
  "removes the %s service registration but keeps host data",
  async (platform) => {
    const home = mkdtempSync(join(tmpdir(), "monocode-service-test-"));
    try {
      const service =
        platform === "darwin"
          ? join(home, "Library/LaunchAgents/com.monocode.host.plist")
          : join(home, ".config/systemd/user/monocode-host.service");
      const data = join(home, ".monocode-host/host.db");
      for (const file of [service, data]) {
        mkdirSync(dirname(file), { recursive: true });
        writeFileSync(file, "existing");
      }
      const calls: string[][] = [];
      const notes = await uninstallService({
        platform,
        home,
        run: async (command, args) => {
          calls.push([command, ...args]);
          // A service that is not loaded must not block cleanup.
          if (["bootout", "disable", "print"].some((arg) => args.includes(arg)))
            throw new Error("not loaded");
        },
      });
      expect(existsSync(service)).toBe(false);
      expect(readFileSync(data, "utf8")).toBe("existing");
      if (platform === "darwin")
        expect(calls[0].slice(0, 2)).toEqual(["launchctl", "bootout"]);
      else {
        expect(calls).toContainEqual([
          "systemctl",
          "--user",
          "disable",
          "--now",
          "monocode-host.service",
        ]);
        expect(notes.join("\n")).toContain("loginctl disable-linger");
      }
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  },
);

it.skipIf(process.platform === "win32")(
  "waits for launchd to finish removing the host before returning",
  async () => {
    const home = mkdtempSync(join(tmpdir(), "monocode-service-test-"));
    try {
      // `bootout` returns while launchd still lists the stopping host.
      let listed = 2;
      const calls: string[] = [];
      await uninstallService({
        platform: "darwin",
        home,
        run: async (_command, args) => {
          calls.push(args[0]);
          if (args[0] === "print" && listed-- <= 0)
            throw new Error("Could not find service");
        },
      });
      expect(calls).toEqual(["bootout", "print", "print", "print"]);
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  },
);

it("unregisters only this user's Windows task", async () => {
  const scripts: string[] = [];
  await uninstallService({
    platform: "win32",
    powershell: async (script) => scripts.push(script),
  });
  expect(scripts[0]).toContain('"MonoCode Host-$sid"');
  expect(scripts[0]).toContain("Unregister-ScheduledTask");
  expect(scripts[0]).not.toMatch(/Remove-Item|\.monocode-host/);
});

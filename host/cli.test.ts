import { expect, it, vi } from "vitest";
import { execFile, spawn } from "node:child_process";
import { promisify } from "node:util";
import { createServer, type AddressInfo } from "node:net";
import {
  mkdtempSync,
  existsSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { lifecycle, readRunning } from "./control";
import { writeNetworkSettings } from "./network";

const exec = promisify(execFile);

it(
  "starts detached, authenticates a device, revokes it, and stops independently of the launcher",
  async () => {
    const directory = mkdtempSync(join(tmpdir(), "monocode-cli-test-"));
    const probe = createServer();
    await new Promise<void>((resolve, reject) => {
      probe.once("error", reject);
      probe.listen(0, "127.0.0.1", resolve);
    });
    const port = (probe.address() as AddressInfo).port;
    await new Promise<void>((resolve) => probe.close(() => resolve()));
    const cli = resolve("build/host/monocode-host.mjs");
    const run = (...args: string[]) =>
      exec(
        process.execPath,
        [cli, ...args, "--data-dir", directory, "--port", String(port)],
        { timeout: process.platform === "win32" ? 25_000 : 10_000 },
      );
    try {
      // A stale legacy PID now belongs to this unrelated test process.
      writeFileSync(join(directory, "owner.lock"), String(process.pid));
      expect((await run("start")).stdout).toContain("Host started");
      expect((await run("status")).stdout).toMatch(/Host [\d.]+ is running/);
      await expect(run("serve")).rejects.toThrow("A host already owns");
      const before = JSON.parse((await run("connection-info")).stdout);
      // Connecting to an existing host must not install a service or restart it.
      expect(JSON.parse((await run("service", "install")).stdout)).toEqual(
        before,
      );
      const device = JSON.parse(
        (await run("pair", "--name", "Test laptop")).stdout,
      ) as { id: string; token: string; environmentId: string };
      const describe = () =>
        fetch(`http://127.0.0.1:${port}/rpc`, {
          method: "POST",
          headers: { Authorization: `Bearer ${device.token}` },
          body: JSON.stringify({
            version: 1,
            method: "environment.describe",
            params: {},
          }),
        });
      const response = await describe();
      expect(response.status).toBe(200);
      expect(
        ((await response.json()) as { result: { environmentId: string } })
          .result.environmentId,
      ).toBe(device.environmentId);
      if (process.platform !== "win32") {
        expect(statSync(directory).mode & 0o777).toBe(0o700);
        expect(statSync(join(directory, "running.json")).mode & 0o777).toBe(
          0o600,
        );
      }
      await run("revoke", device.id);
      await expect(run("revoke", device.id)).rejects.toThrow(
        "Device not found",
      );
      expect((await describe()).status).toBe(401);
      expect((await run("stop")).stdout).toContain("Host is stopping");
      await vi.waitFor(() =>
        expect(existsSync(join(directory, "running.json"))).toBe(false),
      );
      expect((await run("status")).stdout).toContain("Host is stopped");
    } finally {
      if (existsSync(join(directory, "running.json"))) {
        const state = JSON.parse(
          readFileSync(join(directory, "running.json"), "utf8"),
        ) as { pid: number };
        try {
          await run("stop");
        } catch {
          try {
            process.kill(state.pid, "SIGTERM");
          } catch {
            /* gone */
          }
        }
        await vi.waitFor(() =>
          expect(existsSync(join(directory, "running.json"))).toBe(false),
        );
      }
      rmSync(directory, { recursive: true, force: true });
    }
  },
  process.platform === "win32" ? 60_000 : 20_000,
);

it("answers failed network changes, keeps the host alive, and recovers its listener", async () => {
  const directory = mkdtempSync(join(tmpdir(), "monocode-rebind-test-"));
  const probe = createServer();
  await new Promise<void>((resolve) => probe.listen(0, "127.0.0.1", resolve));
  const port = (probe.address() as AddressInfo).port;
  await new Promise<void>((resolve) => probe.close(() => resolve()));
  const blocked = join(directory, "blocked");
  const preload = join(directory, "listen-failure.cjs");
  // Make both the network bind and loopback fallback fail after startup.
  // This exercises the built CLI's event listener and unhandled-rejection policy.
  writeFileSync(
    preload,
    `
      const { Server } = require("node:net");
      const { existsSync } = require("node:fs");
      const listen = Server.prototype.listen;
      Server.prototype.listen = function (...args) {
        if (args[0] === ${port} && existsSync(${JSON.stringify(blocked)})) {
          queueMicrotask(() => this.emit("error", new Error("Injected port conflict")));
          return this;
        }
        return listen.apply(this, args);
      };
    `,
  );
  const child = spawn(
    process.execPath,
    [
      "--require",
      preload,
      resolve("build/host/monocode-host.mjs"),
      "serve",
      "--data-dir",
      directory,
      "--port",
      String(port),
    ],
    { stdio: ["ignore", "ignore", "pipe"] },
  );
  let stderr = "";
  child.stderr.on("data", (chunk) => (stderr += String(chunk)));
  const exited = new Promise<void>((resolve) =>
    child.once("exit", () => resolve()),
  );
  try {
    await vi.waitFor(() => expect(readRunning(directory)).toBeDefined(), {
      timeout: 15_000,
    });
    const state = readRunning(directory)!;
    writeNetworkSettings(directory, { enabled: true, bind: "0.0.0.0" });
    writeFileSync(blocked, "");
    const failed = await lifecycle(state, "network");
    expect(failed).toMatchObject({
      pid: child.pid,
      network: {
        enabled: true,
        bind: "0.0.0.0",
        error: "Injected port conflict",
      },
    });
    expect(stderr).toContain("Could not apply network settings");
    expect(child.exitCode).toBeNull();
    expect(stderr).not.toContain("UnhandledPromiseRejection");
    // Recovery reads the latest settings, so disabling access while the port
    // is blocked must restore only loopback, without a process restart.
    writeNetworkSettings(directory, { enabled: false, bind: "0.0.0.0" });
    rmSync(blocked);
    await vi.waitFor(
      async () => {
        expect(await lifecycle(state, "status")).toMatchObject({
          pid: child.pid,
          network: { enabled: false, bind: "0.0.0.0" },
        });
        expect(
          (await lifecycle(state, "status")).network?.error,
        ).toBeUndefined();
      },
      { timeout: 10_000 },
    );
    await lifecycle(state, "stop");
    await exited;
    expect(child.exitCode).toBe(0);
  } finally {
    if (child.exitCode === null) child.kill("SIGTERM");
    await exited;
    rmSync(directory, { recursive: true, force: true });
  }
}, 45_000);

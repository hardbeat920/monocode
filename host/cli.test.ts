import { expect, it, vi } from "vitest";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { createServer, type AddressInfo } from "node:net";
import {
  mkdtempSync,
  existsSync,
  readFileSync,
  rmSync,
  statSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

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
      expect((await run("start")).stdout).toContain("Host started");
      expect((await run("status")).stdout).toContain("Host is running");
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

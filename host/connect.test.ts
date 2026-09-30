import { afterEach, expect, it } from "vitest";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { request } from "node:https";
import { createServer, type AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import type { TLSSocket } from "node:tls";
import { promisify } from "node:util";
import { parsePairingLink } from "./network";

const exec = promisify(execFile);
const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
});

async function freePort() {
  const probe = createServer();
  await new Promise<void>((resolve) => probe.listen(0, "127.0.0.1", resolve));
  const port = (probe.address() as AddressInfo).port;
  await new Promise<void>((resolve) => probe.close(() => resolve()));
  return port;
}

/** An RPC over TLS that fails unless the host presents the pinned certificate. */
function pinned(
  url: string,
  fingerprint: string,
  payload: unknown,
  token?: string,
): Promise<{ status: number; value: { result?: any; error?: string } }> {
  return new Promise((resolve, reject) => {
    const target = new URL("/rpc", url);
    const call = request(
      {
        host: target.hostname,
        port: target.port,
        path: target.pathname,
        method: "POST",
        // A fresh connection for each call, so every call checks the pin.
        agent: false,
        rejectUnauthorized: false,
        headers: token ? { Authorization: `Bearer ${token}` } : {},
      },
      (response) => {
        const chunks: Buffer[] = [];
        response.on("data", (chunk) => chunks.push(chunk));
        response.on("end", () =>
          resolve({
            status: response.statusCode!,
            value: JSON.parse(Buffer.concat(chunks).toString()),
          }),
        );
      },
    );
    call.on("socket", (socket) =>
      socket.once("secureConnect", () => {
        const seen = createHash("sha256")
          .update((socket as TLSSocket).getPeerX509Certificate()!.raw)
          .digest("base64url");
        if (seen !== fingerprint) call.destroy(new Error("Certificate changed"));
      }),
    );
    call.on("error", reject);
    call.end(JSON.stringify(payload));
  });
}

it(
  "installs, pairs over pinned TLS, reuses, restarts, and limits a host to loopback",
  async () => {
    const directory = mkdtempSync(join(tmpdir(), "monocode-connect-test-"));
    const port = await freePort();
    const cli = resolve("build/host/monocode-host.mjs");
    const run = async (...args: string[]) =>
      (
        await exec(process.execPath, [cli, ...args, "--data-dir", directory], {
          timeout: process.platform === "win32" ? 60_000 : 20_000,
        })
      ).stdout;
    cleanups.push(async () => {
      await run("stop").catch(() => undefined);
      for (let i = 0; i < 50 && existsSync(join(directory, "running.json")); i++)
        await new Promise((resolve) => setTimeout(resolve, 100));
      // Windows can keep the runtime executable open briefly after the host
      // removes running.json. Retry deletion while those handles close.
      rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    });
    // Tests never install a login service; `--no-service` also leaves an
    // existing one alone.
    const connect = async (...args: string[]) =>
      JSON.parse(
        await run("connect", "--no-service", "--json", "--bind", "127.0.0.1", "--port", String(port), ...args),
      );
    const status = async () => JSON.parse(await run("connect", "status", "--json"));

    const first = await connect();
    expect(first).toMatchObject({
      port,
      service: "detached",
      endpoints: [`https://127.0.0.1:${port}`],
    });
    const offer = parsePairingLink(first.link);
    expect(offer).toMatchObject({
      environmentId: first.environmentId,
      fingerprint: first.fingerprint,
      endpoints: first.endpoints,
    });
    expect(existsSync(join(directory, "runtime", first.version, "monocode-host.mjs"))).toBe(true);
    expect(
      existsSync(
        join(directory, "bin", process.platform === "win32" ? "monocode-host.cmd" : "monocode-host"),
      ),
    ).toBe(true);

    const paired = await pinned(offer.endpoints[0], offer.fingerprint, {
      version: 1,
      method: "pair.exchange",
      params: { code: offer.code, name: "Test desktop" },
    });
    expect(paired.status).toBe(200);
    const token = paired.value.result.token as string;
    const describe = () =>
      pinned(
        offer.endpoints[0],
        offer.fingerprint,
        { version: 1, method: "environment.describe", params: {} },
        token,
      );
    expect((await describe()).value.result).toMatchObject({
      environmentId: offer.environmentId,
      hostVersion: first.version,
      endpoints: [`https://127.0.0.1:${port}`],
    });
    await expect(
      pinned(offer.endpoints[0], "a".repeat(43), { version: 1, method: "environment.describe" }, token),
    ).rejects.toThrow("Certificate changed");

    // Running connect again reuses the running host and issues a new link.
    const pid = (await status()).pid;
    const again = await connect();
    expect(again.link).not.toBe(first.link);
    expect((await status()).pid).toBe(pid);

    const restarted = await connect("--restart", "--yes");
    expect(restarted.fingerprint).toBe(first.fingerprint);
    const after = await status();
    expect(after.pid).not.toBe(pid);
    expect(after.devices.map((device: { name: string }) => device.name)).toEqual([
      "Test desktop",
    ]);
    expect((await describe()).status).toBe(200);

    expect(await run("connect", "disable")).toContain("Network access is off");
    expect(await status()).toMatchObject({
      running: true,
      network: { enabled: false },
      endpoints: [],
    });
    expect((await describe()).value.result.endpoints).toEqual([]);
  },
  90_000,
);

import { afterEach, expect, it } from "vitest";
import { createHash } from "node:crypto";
import { createServer, request as httpRequest } from "node:http";
import { request as httpsRequest } from "node:https";
import type { AddressInfo, Server } from "node:net";
import type { TLSSocket } from "node:tls";
import { isLoopback, listenHost } from "./listener";
import { createHostCertificate } from "./tls";

const identity = createHostCertificate();
const servers: Server[] = [];
afterEach(() => {
  for (const server of servers.splice(0)) server.close();
});

async function start(loopback?: (address: string | undefined) => boolean) {
  const app = createServer((request, response) => {
    const hash = createHash("sha256");
    let size = 0;
    request.on("data", (chunk: Buffer) => {
      size += chunk.length;
      hash.update(chunk);
    });
    request.on("end", () =>
      response.end(
        JSON.stringify({
          size,
          hash: hash.digest("hex"),
          encrypted: !!(request.socket as TLSSocket).encrypted,
        }),
      ),
    );
  });
  const front = await listenHost(app, {
    port: 0,
    bind: "127.0.0.1",
    identity,
    loopback,
  });
  servers.push(front, app);
  return (front.address() as AddressInfo).port;
}

type Reply = { status?: number; body?: { size: number; hash: string; encrypted: boolean }; fingerprint?: string; error?: string };

function send(port: number, secure: boolean, body: Buffer): Promise<Reply> {
  return new Promise((resolve) => {
    const options = {
      host: "127.0.0.1",
      port,
      method: "POST",
      path: "/rpc",
      rejectUnauthorized: false,
    };
    const request = (secure ? httpsRequest : httpRequest)(options, (response) => {
      const fingerprint = secure
        ? createHash("sha256")
            .update((response.socket as TLSSocket).getPeerX509Certificate()!.raw)
            .digest("base64url")
        : undefined;
      const chunks: Buffer[] = [];
      response.on("data", (chunk) => chunks.push(chunk));
      response.on("end", () =>
        resolve({
          status: response.statusCode,
          body: JSON.parse(Buffer.concat(chunks).toString()),
          fingerprint,
        }),
      );
    });
    request.on("error", (error) => resolve({ error: error.message }));
    request.end(body);
  });
}

it("serves plain HTTP to loopback and TLS to everyone on one port", async () => {
  const port = await start();
  const body = Buffer.alloc(3 * 1024 * 1024, 7);
  const digest = createHash("sha256").update(body).digest("hex");
  const plain = await send(port, false, body);
  expect(plain.body).toEqual({ size: body.length, hash: digest, encrypted: false });
  const secure = await send(port, true, body);
  expect(secure.body).toEqual({ size: body.length, hash: digest, encrypted: true });
  expect(secure.fingerprint).toBe(identity.fingerprint);
});

it("refuses plain HTTP from another computer", async () => {
  const port = await start(() => false);
  expect((await send(port, false, Buffer.from("{}"))).error).toBeTruthy();
  expect((await send(port, true, Buffer.from("{}"))).body?.encrypted).toBe(true);
});

it("recognizes IPv4, IPv6, and mapped loopback addresses", () => {
  for (const address of ["127.0.0.1", "127.1.2.3", "::1", "::ffff:127.0.0.1"])
    expect(isLoopback(address)).toBe(true);
  for (const address of ["10.0.0.5", "::ffff:10.0.0.5", "fe80::1", undefined])
    expect(isLoopback(address)).toBe(false);
});

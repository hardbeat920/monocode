import { afterEach, expect, it } from "vitest";
import { createHash, X509Certificate } from "node:crypto";
import { mkdtempSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { connect, createServer } from "node:tls";
import type { AddressInfo } from "node:net";
import {
  certificateFingerprint,
  createHostCertificate,
  loadHostIdentity,
} from "./tls";

const directories: string[] = [];
afterEach(() => {
  for (const directory of directories.splice(0))
    rmSync(directory, { recursive: true, force: true });
});

it("creates a valid self-signed certificate that a TLS server accepts", async () => {
  const identity = createHostCertificate("Test host", new Date("2026-01-01"));
  const certificate = new X509Certificate(identity.cert);
  expect(certificate.subject).toBe("CN=Test host");
  expect(certificate.verify(certificate.publicKey)).toBe(true);
  expect(certificate.subjectAltName).toBe("DNS:monocode-host");
  expect(certificate.ca).toBe(false);
  expect(new Date(certificate.validTo).getUTCFullYear()).toBe(2045);
  expect(identity.fingerprint).toBe(
    createHash("sha256").update(certificate.raw).digest("base64url"),
  );
  expect(identity.fingerprint).toMatch(/^[\w-]{43}$/);

  const server = createServer({ key: identity.key, cert: identity.cert }, (socket) =>
    socket.end("hello"),
  );
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    const seen = await new Promise<string>((resolve, reject) => {
      const socket = connect({
        host: "127.0.0.1",
        port: (server.address() as AddressInfo).port,
        rejectUnauthorized: false,
      });
      socket.once("secureConnect", () =>
        resolve(
          createHash("sha256")
            .update(socket.getPeerX509Certificate()!.raw)
            .digest("base64url"),
        ),
      );
      socket.once("error", reject);
    });
    expect(seen).toBe(identity.fingerprint);
  } finally {
    server.close();
  }
});

it("uses GeneralizedTime for validity dates from 2050", () => {
  const identity = createHostCertificate("Later", new Date("2040-06-01"));
  const certificate = new X509Certificate(identity.cert);
  expect(new Date(certificate.validTo).getUTCFullYear()).toBe(2060);
  expect(certificateFingerprint(identity.cert)).toBe(identity.fingerprint);
});

it("creates one identity per data directory and reuses it", () => {
  const directory = mkdtempSync(join(tmpdir(), "monocode-tls-"));
  directories.push(directory);
  const first = loadHostIdentity(directory);
  const second = loadHostIdentity(directory);
  expect(second).toEqual(first);
  if (process.platform !== "win32")
    expect(statSync(join(directory, "tls", "key.pem")).mode & 0o077).toBe(0);
});

import {
  createHash,
  generateKeyPairSync,
  randomBytes,
  sign,
  X509Certificate,
} from "node:crypto";
import { mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

/** The host's TLS identity. Desktops pin `fingerprint`, the SHA-256 of the
 * certificate, so the certificate needs no public CA or hostname. */
export type HostIdentity = { key: string; cert: string; fingerprint: string };

// Minimal DER encoding: enough for one self-signed X.509 v3 certificate.
const length = (size: number): Buffer => {
  if (size < 0x80) return Buffer.from([size]);
  const bytes: number[] = [];
  for (let value = size; value > 0; value = Math.floor(value / 256))
    bytes.unshift(value & 0xff);
  return Buffer.from([0x80 | bytes.length, ...bytes]);
};
const tlv = (tag: number, ...content: Buffer[]): Buffer => {
  const body = Buffer.concat(content);
  return Buffer.concat([Buffer.from([tag]), length(body.length), body]);
};
const sequence = (...content: Buffer[]) => tlv(0x30, ...content);
const oid = (value: string): Buffer => {
  const [first, second, ...rest] = value.split(".").map(Number);
  const bytes = [first * 40 + second];
  for (const part of rest) {
    const chunk = [part & 0x7f];
    for (let rem = Math.floor(part / 128); rem > 0; rem = Math.floor(rem / 128))
      chunk.unshift((rem & 0x7f) | 0x80);
    bytes.push(...chunk);
  }
  return tlv(0x06, Buffer.from(bytes));
};
const time = (date: Date): Buffer => {
  const iso = date.toISOString().replace(/[-:T]/g, "").slice(0, 14) + "Z";
  // RFC 5280: UTCTime through 2049, GeneralizedTime from 2050.
  return date.getUTCFullYear() < 2050
    ? tlv(0x17, Buffer.from(iso.slice(2)))
    : tlv(0x18, Buffer.from(iso));
};

const ECDSA_SHA256 = sequence(oid("1.2.840.10045.4.3.2"));

export function createHostCertificate(
  commonName = "MonoCode Host",
  now = new Date(),
): HostIdentity {
  const { privateKey, publicKey } = generateKeyPairSync("ec", {
    namedCurve: "prime256v1",
  });
  const serial = randomBytes(16);
  serial[0] &= 0x7f; // a positive INTEGER
  serial[0] |= 0x01; // with no leading zero byte
  const name = sequence(
    tlv(0x31, sequence(oid("2.5.4.3"), tlv(0x0c, Buffer.from(commonName)))),
  );
  const extensions = tlv(
    0xa3,
    sequence(
      // basicConstraints: not a CA
      sequence(oid("2.5.29.19"), tlv(0x04, sequence())),
      // subjectAltName: dNSName monocode-host
      sequence(
        oid("2.5.29.17"),
        tlv(0x04, sequence(tlv(0x82, Buffer.from("monocode-host")))),
      ),
    ),
  );
  const tbs = sequence(
    tlv(0xa0, tlv(0x02, Buffer.from([2]))),
    tlv(0x02, serial),
    ECDSA_SHA256,
    name,
    sequence(
      time(new Date(now.getTime() - 24 * 3600_000)),
      time(new Date(now.getTime() + 20 * 365 * 24 * 3600_000)),
    ),
    name,
    publicKey.export({ type: "spki", format: "der" }),
    extensions,
  );
  const signature = sign("sha256", tbs, privateKey);
  const der = sequence(
    tbs,
    ECDSA_SHA256,
    tlv(0x03, Buffer.from([0]), signature),
  );
  const cert = `-----BEGIN CERTIFICATE-----\n${der
    .toString("base64")
    .replace(/.{64}/g, "$&\n")
    .trim()}\n-----END CERTIFICATE-----\n`;
  return {
    key: String(privateKey.export({ type: "pkcs8", format: "pem" })),
    cert,
    fingerprint: certificateFingerprint(cert),
  };
}

export function certificateFingerprint(pem: string): string {
  return createHash("sha256")
    .update(new X509Certificate(pem).raw)
    .digest("base64url");
}

/**
 * Reads the identity in `<directory>/tls`, creating it on first use. The CLI
 * and a starting host may race; each writes a complete temporary directory
 * and only one rename wins, so both end up with the same certificate.
 */
export function loadHostIdentity(directory: string): HostIdentity {
  const folder = join(directory, "tls");
  const read = (): HostIdentity => {
    const cert = readFileSync(join(folder, "cert.pem"), "utf8");
    return {
      key: readFileSync(join(folder, "key.pem"), "utf8"),
      cert,
      fingerprint: certificateFingerprint(cert),
    };
  };
  try {
    return read();
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
  }
  const identity = createHostCertificate();
  const temporary = mkdtempSync(join(directory, ".tls-"));
  try {
    writeFileSync(join(temporary, "key.pem"), identity.key, { mode: 0o600 });
    writeFileSync(join(temporary, "cert.pem"), identity.cert, { mode: 0o600 });
    renameSync(temporary, folder);
    return identity;
  } catch (error) {
    rmSync(temporary, { recursive: true, force: true });
    const code = (error as NodeJS.ErrnoException).code;
    if (code === "EEXIST" || code === "ENOTEMPTY" || code === "EPERM")
      return read();
    throw error;
  }
}

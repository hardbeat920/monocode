import { createServer, type Server as NetServer, type Socket } from "node:net";
import type { Server as HttpServer } from "node:http";
import { createServer as createTlsServer } from "node:tls";
import type { HostIdentity } from "./tls";

export const isLoopback = (address: string | undefined) =>
  !!address &&
  (/^127\./.test(address) ||
    address === "::1" ||
    /^::ffff:127\./i.test(address));

// A TLS connection opens with a handshake record; plain HTTP opens with a
// method name.
const TLS_HANDSHAKE = 0x16;

export type HostListenerOptions = {
  port: number;
  /** `127.0.0.1` for loopback only, or an address such as `0.0.0.0`. */
  bind: string;
  /** Enables TLS. Without it, only loopback clients are served. */
  identity?: HostIdentity;
  /** Overridable in tests, which cannot open a non-loopback connection. */
  loopback?: (address: string | undefined) => boolean;
};

/**
 * Serves `server` on one port. Loopback clients (this machine's CLI, or an SSH
 * forward) may use plain HTTP. Every other client must use TLS; desktops pin
 * the certificate fingerprint they received in the pairing link.
 */
export function listenHost(
  server: HttpServer,
  options: HostListenerOptions,
): Promise<NetServer> {
  const loopback = options.loopback ?? isLoopback;
  const tls = options.identity
    ? createTlsServer(
        {
          key: options.identity.key,
          cert: options.identity.cert,
          minVersion: "TLSv1.2",
          handshakeTimeout: 10_000,
        },
        (socket) => server.emit("connection", socket),
      ).on("tlsClientError", (_error, socket) => socket.destroy())
    : undefined;
  const front = createServer((socket: Socket) => {
    const local = loopback(socket.remoteAddress);
    socket.on("error", () => socket.destroy());
    socket.setTimeout(10_000, () => socket.destroy());
    socket.once("data", (chunk: Buffer) => {
      socket.setTimeout(0);
      socket.pause();
      socket.unshift(chunk);
      if (tls && chunk[0] === TLS_HANDSHAKE) tls.emit("connection", socket);
      else if (local) server.emit("connection", socket);
      else {
        socket.destroy();
        return;
      }
      process.nextTick(() => socket.resume());
    });
  });
  front.on("close", () => tls?.close());
  return new Promise((resolve, reject) => {
    front.once("error", reject);
    front.listen(options.port, options.bind, () => {
      front.off("error", reject);
      resolve(front);
    });
  });
}

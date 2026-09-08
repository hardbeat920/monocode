import { describe, expect, it, vi } from "vitest";
import {
  claimPairingCode,
  RemoteTransport,
  type WebSocketLike,
} from "./remote";

type Harness = {
  socket: WebSocketLike;
  received: string[];
  url: string;
  serverSend: (frame: unknown) => void;
  serverClose: (code?: number, reason?: string) => void;
};

function installFakeSocket(): {
  factory: (url: string) => WebSocketLike;
  harness: () => Harness;
} {
  let current: Harness | null = null;
  const factory = (url: string) => {
    const received: string[] = [];
    const socket: WebSocketLike = {
      send: (data: string) => {
        received.push(data);
      },
      close: (code = 1000, reason = "test close") => {
        socket.onclose?.({ code, reason });
      },
      onopen: null,
      onmessage: null,
      onclose: null,
      onerror: null,
    };
    const harness: Harness = {
      socket,
      received,
      url,
      serverSend: (frame: unknown) => {
        socket.onmessage?.({ data: JSON.stringify(frame) });
      },
      serverClose: (code = 1000, reason = "test close") => {
        socket.onclose?.({ code, reason });
      },
    };
    current = harness;
    return socket;
  };
  return {
    factory,
    harness: () => {
      if (!current) throw new Error("socket not created yet");
      return current;
    },
  };
}

function openSocket(harness: Harness): void {
  harness.socket.onopen?.({});
}

describe("claimPairingCode", () => {
  function claimSocket(
    behavior: (socket: WebSocketLike, sent: string[]) => void,
  ): (url: string) => WebSocketLike {
    return () => {
      const sent: string[] = [];
      const socket: WebSocketLike = {
        send: (data: string) => {
          sent.push(data);
          behavior(socket, sent);
        },
        close: () => {},
        onopen: null,
        onmessage: null,
        onclose: null,
        onerror: null,
      };
      queueMicrotask(() => socket.onopen?.({}));
      return socket;
    };
  }

  it("rejects malformed codes without dialing", async () => {
    let dials = 0;
    await expect(
      claimPairingCode({ host: "h", port: 1 }, "12", {
        socket: () => {
          dials += 1;
          throw new Error("unreachable");
        },
      }),
    ).rejects.toThrow("6-digit");
    expect(dials).toBe(0);
  });

  it("keeps advertised LAN and Tailscale hosts from the claim payload", async () => {
    const factory = claimSocket((socket) => {
      socket.onmessage?.({
        data: JSON.stringify({
          id: 1,
          type: "result",
          ok: true,
          payload: {
            token: "real-token-value",
            lanIp: "192.168.1.20",
            tailnetHost: "mac.tail.ts.net",
          },
        }),
      });
    });
    await expect(
      claimPairingCode({ host: "mac", port: 17233 }, "123 456", {
        socket: factory,
      }),
    ).resolves.toEqual({
      token: "real-token-value",
      lanIp: "192.168.1.20",
      tailnetHost: "mac.tail.ts.net",
    });
  });

  it("exchanges the code for a token", async () => {
    const factory = claimSocket((socket) => {
      socket.onmessage?.({
        data: JSON.stringify({
          id: 1,
          type: "result",
          ok: true,
          payload: { token: "real-token-value" },
        }),
      });
    });
    await expect(
      claimPairingCode({ host: "mac", port: 17233 }, "123 456", {
        socket: factory,
      }),
    ).resolves.toEqual({ token: "real-token-value" });
  });

  it("surfaces host rejections (wrong/expired code)", async () => {
    const factory = claimSocket((socket) => {
      socket.onmessage?.({
        data: JSON.stringify({
          id: 1,
          type: "result",
          ok: false,
          error: "Wrong code — check the host screen and retry.",
        }),
      });
    });
    await expect(
      claimPairingCode({ host: "mac", port: 17233 }, "000000", {
        socket: factory,
      }),
    ).rejects.toThrow("Wrong code");
  });

  it("falls back from ws to wss on the same host:port", async () => {
    const dialed: string[] = [];
    const factory = (url: string) => {
      dialed.push(url);
      if (url.startsWith("ws://")) {
        return claimSocket((socket) => {
          socket.onerror?.({});
        })("");
      }
      return claimSocket((socket) => {
        socket.onmessage?.({
          data: JSON.stringify({
            id: 1,
            type: "result",
            ok: true,
            payload: { token: "t" },
          }),
        });
      })("");
    };
    await expect(
      claimPairingCode({ host: "mac", port: 17233 }, "123456", {
        socket: factory,
      }),
    ).resolves.toEqual({ token: "t" });
    expect(dialed).toEqual([
      "ws://mac:17233/v1/connect?pair=1",
      "wss://mac:17233/v1/connect?pair=1",
    ]);
  });
});

describe("RemoteTransport", () => {
  it("resolves invokes from host result frames", async () => {
    const { factory, harness } = installFakeSocket();
    const transport = new RemoteTransport("ws://host:17233/v1/connect", {
      socket: factory,
      reconnect: false,
    });
    const connected = transport.connect();
    openSocket(harness());
    await connected;

    const pending = transport.invoke<number>("harness_free_port");
    const sent = JSON.parse(harness().received[0] ?? "{}") as {
      id: number;
      type: string;
      command: string;
    };
    expect(sent.type).toBe("invoke");
    expect(sent.command).toBe("harness_free_port");
    harness().serverSend({ id: sent.id, type: "result", ok: true, payload: 54321 });
    await expect(pending).resolves.toBe(54321);
    transport.dispose();
  });

  it("rejects invokes on host error frames", async () => {
    const { factory, harness } = installFakeSocket();
    const transport = new RemoteTransport("ws://host:17233/v1/connect", {
      socket: factory,
      reconnect: false,
    });
    const connected = transport.connect();
    openSocket(harness());
    await connected;

    const pending = transport.invoke("pty_spawn", { id: "t1" });
    const sent = JSON.parse(harness().received[0] ?? "{}") as { id: number };
    harness().serverSend({
      id: sent.id,
      type: "result",
      ok: false,
      error: "Terminal is not running",
    });
    await expect(pending).rejects.toThrow("Terminal is not running");
    transport.dispose();
  });

  it("fans out host events to matching listeners only", async () => {
    const { factory, harness } = installFakeSocket();
    const transport = new RemoteTransport("ws://host:17233/v1/connect", {
      socket: factory,
      reconnect: false,
    });
    const connected = transport.connect();
    openSocket(harness());
    await connected;

    const seen: string[] = [];
    const other: string[] = [];
    const unlisten = await transport.listen<{ line: string }>(
      "harness-stdout",
      (event) => {
        seen.push(event.payload.line);
      },
    );
    await transport.listen<{ line: string }>("harness-stderr", (event) => {
      other.push(event.payload.line);
    });
    harness().serverSend({
      type: "event",
      event: "harness-stdout",
      payload: { sessionId: "s1", line: "hello" },
    });
    harness().serverSend({
      type: "event",
      event: "harness-stderr",
      payload: { sessionId: "s1", line: "warn" },
    });
    expect(seen).toEqual(["hello"]);
    expect(other).toEqual(["warn"]);

    unlisten();
    harness().serverSend({
      type: "event",
      event: "harness-stdout",
      payload: { sessionId: "s1", line: "after unlisten" },
    });
    expect(seen).toEqual(["hello"]);
    transport.dispose();
  });

  it("rejects in-flight invokes when the socket closes", async () => {
    const { factory, harness } = installFakeSocket();
    const transport = new RemoteTransport("ws://host:17233/v1/connect", {
      socket: factory,
      reconnect: false,
    });
    const connected = transport.connect();
    openSocket(harness());
    await connected;

    const pending = transport.invoke("session_upsert", { id: "s1" });
    const failure = expect(pending).rejects.toThrow();
    harness().serverClose(1006);
    await failure;
    transport.dispose();
  });

  it("times out invokes that the host never answers", async () => {
    const { factory, harness } = installFakeSocket();
    const transport = new RemoteTransport("ws://host:17233/v1/connect", {
      socket: factory,
      reconnect: false,
      invokeTimeoutMs: 20,
    });
    const connected = transport.connect();
    openSocket(harness());
    await connected;

    await expect(transport.invoke("pty_spawn")).rejects.toThrow("timed out");
    transport.dispose();
  });

  it("reports status transitions to observers", async () => {
    const { factory, harness } = installFakeSocket();
    const transport = new RemoteTransport("ws://host:17233/v1/connect", {
      socket: factory,
      reconnect: false,
    });
    const statuses: string[] = [];
    transport.onStatusChange((status) => statuses.push(status));
    const connected = transport.connect();
    openSocket(harness());
    await connected;
    transport.disconnect();
    expect(statuses).toContain("open");
    expect(statuses[statuses.length - 1]).toBe("closed");
    transport.dispose();
  });

  it("connect rejects when the host refuses the dial", async () => {
    const { factory, harness } = installFakeSocket();
    const transport = new RemoteTransport("ws://host:17233/v1/connect", {
      socket: factory,
      reconnect: false,
    });
    const connected = transport.connect();
    const failure = expect(connected).rejects.toThrow();
    harness().serverClose(1006);
    await failure;
    transport.dispose();
  });

  it("ignores malformed frames without breaking the link", async () => {
    const { factory, harness } = installFakeSocket();
    const transport = new RemoteTransport("ws://host:17233/v1/connect", {
      socket: factory,
      reconnect: false,
    });
    const connected = transport.connect();
    openSocket(harness());
    await connected;

    const onData = vi.fn();
    await transport.listen("pty-data", onData);
    harness().socket.onmessage?.({ data: "not-json{{{" });
    harness().socket.onmessage?.({ data: "42" });
    harness().serverSend({
      type: "event",
      event: "pty-data",
      payload: { id: "t1", data: "aGk=" },
    });
    expect(onData).toHaveBeenCalledTimes(1);
    transport.dispose();
  });

  it("fails over to the next url when the first dial times out", async () => {
    const sockets: Array<{ url: string; socket: WebSocketLike }> = [];
    const factory = (url: string) => {
      const socket: WebSocketLike = {
        send: () => {},
        close: (code = 1000, reason = "test close") => {
          socket.onclose?.({ code, reason });
        },
        onopen: null,
        onmessage: null,
        onclose: null,
        onerror: null,
      };
      sockets.push({ url, socket });
      return socket;
    };
    const transport = new RemoteTransport(
      [
        "ws://100.80.151.26:17233/v1/connect",
        "ws://192.168.4.191:17233/v1/connect",
      ],
      { socket: factory, reconnect: false, connectTimeoutMs: 30 },
    );
    const connected = transport.connect();
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(sockets.map((item) => item.url)).toEqual([
      "ws://100.80.151.26:17233/v1/connect",
      "ws://192.168.4.191:17233/v1/connect",
    ]);
    sockets[1]?.socket.onopen?.({});
    await connected;
    transport.dispose();
  });

  it("does not reconnect after a 401", async () => {
    let dials = 0;
    const { factory, harness } = installFakeSocket();
    const transport = new RemoteTransport("ws://host:17233/v1/connect", {
      socket: (url) => {
        dials += 1;
        return factory(url);
      },
      reconnect: true,
      connectTimeoutMs: 200,
      maxBackoffMs: 20,
    });
    const connected = transport.connect();
    harness().serverClose(
      1006,
      "websocket handshake failed: HTTP error: 401 Unauthorized",
    );
    await expect(connected).rejects.toThrow(/pairing token rejected/);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(dials).toBe(1);
    transport.dispose();
  });
});

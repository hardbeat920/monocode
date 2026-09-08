import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearPairing,
  connectAndRememberCompanion,
  connectCompanion,
  disconnectCompanion,
  forgetCompanion,
  getCompanionStatus,
  isRemote,
  loadCompanionMode,
  loadPairing,
  reconnectCompanion,
  savePairing,
  setCompanionMode,
  switchCompanionRoute,
  waitForCompanionLink,
} from "./index";

function installMemoryStorage(): void {
  const store = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => {
      store.set(key, value);
    },
    removeItem: (key: string) => {
      store.delete(key);
    },
  });
}

beforeEach(() => {
  disconnectCompanion();
  clearPairing();
  setCompanionMode(false);
  installMemoryStorage();
});

describe("companion link state", () => {
  it("persists pairing details across loads", () => {
    expect(loadPairing()).toBeNull();
    savePairing({ host: "macbook", port: 17233, token: "0123456789abcdef" });
    expect(loadPairing()).toEqual({
      host: "macbook",
      port: 17233,
      token: "0123456789abcdef",
    });
  });

  it("persists an alternate host and switchCompanionRoute swaps it", () => {
    const inertSocket = () => ({
      send: () => {},
      close: () => {},
      onopen: null,
      onmessage: null,
      onclose: null,
      onerror: null,
    });
    savePairing({
      host: "192.168.1.20",
      port: 17233,
      token: "0123456789abcdef",
      altHost: "mac.tail.ts.net",
    });
    expect(loadPairing()?.altHost).toBe("mac.tail.ts.net");
    setCompanionMode(true);
    const transport = switchCompanionRoute({
      reconnect: false,
      socket: inertSocket,
    });
    expect(transport).not.toBeNull();
    expect(loadPairing()).toEqual({
      host: "mac.tail.ts.net",
      port: 17233,
      token: "0123456789abcdef",
      altHost: "192.168.1.20",
    });
    transport?.dispose();
    forgetCompanion();
  });

  it("persists the secure flag and can drop it", () => {
    savePairing({
      host: "macbook.tail.ts.net",
      port: 443,
      token: "0123456789abcdef",
      secure: true,
    });
    expect(loadPairing()).toEqual({
      host: "macbook.tail.ts.net",
      port: 443,
      token: "0123456789abcdef",
      secure: true,
    });
    savePairing({ host: "192.168.1.20", port: 17233, token: "0123456789abcdef" });
    expect(loadPairing()?.secure).toBeUndefined();
  });

  it("keeps companion mode separate from pairing", () => {
    expect(loadCompanionMode()).toBe(false);
    setCompanionMode(true);
    expect(loadCompanionMode()).toBe(true);
    setCompanionMode(false);
    expect(loadCompanionMode()).toBe(false);
  });

  it("connectAndRemember flips to remote and forget restores local", () => {
    expect(isRemote()).toBe(false);
    // Never open, never close: no network, deterministic teardown.
    const inertSocket = () => ({
      send: () => {},
      close: () => {},
      onopen: null,
      onmessage: null,
      onclose: null,
      onerror: null,
    });
    const transport = connectAndRememberCompanion(
      { host: "127.0.0.1", port: 1, token: "0123456789abcdef" },
      { reconnect: false, socket: inertSocket },
    );
    expect(isRemote()).toBe(true);
    expect(loadCompanionMode()).toBe(true);
    expect(loadPairing()).toEqual({
      host: "127.0.0.1",
      port: 1,
      token: "0123456789abcdef",
    });
    transport.dispose();

    forgetCompanion();
    expect(isRemote()).toBe(false);
    expect(loadCompanionMode()).toBe(false);
    expect(loadPairing()).toBeNull();
    expect(getCompanionStatus()).toBe("local");
  });

  it("disconnect keeps pairing and reconnect redials it", () => {
    const inertSocket = () => ({
      send: () => {},
      close: () => {},
      onopen: null,
      onmessage: null,
      onclose: null,
      onerror: null,
    });
    savePairing({ host: "mac.local", port: 17233, token: "0123456789abcdef" });
    setCompanionMode(true);
    connectCompanion(
      { host: "mac.local", port: 17233, token: "0123456789abcdef" },
      { reconnect: false, socket: inertSocket },
    );
    expect(isRemote()).toBe(true);

    disconnectCompanion();
    expect(isRemote()).toBe(false);
    expect(loadCompanionMode()).toBe(true);
    expect(loadPairing()).toEqual({
      host: "mac.local",
      port: 17233,
      token: "0123456789abcdef",
    });
    expect(getCompanionStatus()).toBe("local");

    const transport = reconnectCompanion({
      reconnect: false,
      socket: inertSocket,
    });
    expect(transport).not.toBeNull();
    expect(isRemote()).toBe(true);
    transport?.dispose();
    forgetCompanion();
    expect(reconnectCompanion({ reconnect: false, socket: inertSocket })).toBeNull();
  });

  it("waitForCompanionLink resolves immediately on desktop installs", async () => {
    setCompanionMode(false);
    await expect(waitForCompanionLink(10)).resolves.toBe(true);
  });

  it("waitForCompanionLink resolves immediately with no saved pairing", async () => {
    setCompanionMode(true);
    clearPairing();
    await expect(waitForCompanionLink(10)).resolves.toBe(true);
  });
});

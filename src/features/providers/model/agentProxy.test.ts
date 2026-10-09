import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { loadAgentProxyUrl, saveAgentProxyUrl } from "./agentProxy";

beforeEach(() => {
  const values = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
  });
});

afterEach(() => vi.unstubAllGlobals());

describe("agent proxy preference", () => {
  it("persists a custom HTTP proxy and restores inherited networking when cleared", () => {
    expect(loadAgentProxyUrl()).toBeNull();
    expect(saveAgentProxyUrl("http://127.0.0.1:7897")).toBe(true);
    expect(loadAgentProxyUrl()).toBe("http://127.0.0.1:7897");
    expect(saveAgentProxyUrl(null)).toBe(true);
    expect(loadAgentProxyUrl()).toBeNull();
  });

  it("rejects invalid URLs and credentials without replacing a saved proxy", () => {
    saveAgentProxyUrl("http://127.0.0.1:7897");
    expect(saveAgentProxyUrl("file:///tmp/proxy")).toBe(false);
    expect(saveAgentProxyUrl("http://user:password@127.0.0.1:7897")).toBe(false);
    expect(loadAgentProxyUrl()).toBe("http://127.0.0.1:7897");
  });
});

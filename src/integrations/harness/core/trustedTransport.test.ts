import { describe, expect, it } from "vitest";
import {
  namespaceTransportChild,
  validateTrustedTransport,
} from "./trustedTransport";

describe("trusted transport boundary", () => {
  it("copies resolver-owned descriptors and keeps argv fixed by value", () => {
    const args = ["acp"];
    const descriptor = validateTrustedTransport({
      provider: "hermes",
      path: "/usr/local/bin/hermes",
      args,
    });
    args[0] = "--shell";
    expect(descriptor).toEqual({
      provider: "hermes",
      path: "/usr/local/bin/hermes",
      args: ["acp"],
    });
  });

  it.each([
    { provider: "", path: "/bin/agent", args: [] },
    { provider: "agent", path: "", args: [] },
    { provider: "agent", path: "/bin/agent\0evil", args: [] },
    { provider: "agent", path: "/bin/agent", args: ["ok\0evil"] },
  ])("rejects malformed descriptor %#", (descriptor) => {
    expect(() => validateTrustedTransport(descriptor)).toThrow();
  });

  it("namespaces runtime children without changing provider identity", () => {
    expect(namespaceTransportChild("acp", "thread", 3)).toBe("acp:thread:3");
    expect(namespaceTransportChild("acp", "thread", 4)).not.toBe(
      namespaceTransportChild("acp", "thread", 3),
    );
  });

  it.each([
    ["", "thread", 0],
    ["acp", "thread#1", 0],
    ["acp", "thread", -1],
    ["acp", "thread", 1.5],
  ])("rejects invalid runtime child %#", (provider, session, generation) => {
    expect(() => namespaceTransportChild(provider, session, generation)).toThrow();
  });
});

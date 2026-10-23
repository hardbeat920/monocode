import { describe, expect, it } from "vitest";
import { hostUpdateAvailable, isOlderHostVersion } from "./hostUpdates";
import type { HostDescriptor } from "./protocol";

describe("host package versions", () => {
  it.each([
    ["0.7.1", "0.13.0", true],
    ["0.9.0", "0.10.0", true],
    ["0.13.0", "0.13.0", false],
    ["1.0.0", "0.13.0", false],
    ["v0.12.0\n", "0.13.0", true],
    ["0.13.0-rc.2", "0.13.0-rc.10", true],
    ["0.13.0-rc.10", "0.13.0-rc.2", false],
    ["0.13.0-rc.1", "0.13.0", true],
    ["0.13.0", "0.13.0-rc.1", false],
    ["0.14.0-alpha", "0.13.0", false],
    ["0.13.0-alpha", "0.13.0-alpha.1", true],
    ["0.13.0-1", "0.13.0-alpha", true],
    ["0.13.0-beta", "0.13.0-alpha", false],
    ["0.13.0+build.1", "0.13.0+build.2", false],
    ["0.13.0-rc.1+build.1", "0.13.0-rc.1+build.2", false],
    ["0.13.0-9007199254740992", "0.13.0-9007199254740993", true],
    [undefined, "0.13.0", false],
    ["", "0.13.0", false],
    ["unknown", "0.13.0", false],
    ["0.1", "0.13.0", false],
    ["00.1.0", "0.13.0", false],
    ["0.1.0-01", "0.13.0", false],
    ["0.1.0", "broken-client-version", false],
    ["0.1.0-extra..part", "0.13.0", false],
  ])("%s compared with %s offers an update: %s", (host, client, expected) => {
    expect(isOlderHostVersion(host, client)).toBe(expected);
  });

  it("retains capability upgrades when versions are absent, equal or newer", () => {
    for (const hostVersion of [undefined, "0.13.0", "99.0.0"]) {
      const descriptor = {
        hostVersion,
        capabilities: ["workspace.run"],
      } as HostDescriptor;
      expect(hostUpdateAvailable(descriptor)).toBe(true);
      descriptor.capabilities.push("git.worktreeCreate");
      expect(hostUpdateAvailable(descriptor)).toBe(false);
    }
  });
});

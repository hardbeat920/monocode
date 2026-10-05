import { afterEach, expect, it } from "vitest";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { installRuntime, pruneRuntimes, stableNodePath } from "./runtime";
import {
  networkEndpoints,
  pairingLink,
  parsePairingLink,
} from "./network";

const directories: string[] = [];
afterEach(() => {
  for (const directory of directories.splice(0))
    rmSync(directory, { recursive: true, force: true });
});
const temporary = () => {
  const directory = mkdtempSync(join(tmpdir(), "monocode-runtime-"));
  directories.push(directory);
  return directory;
};

it("copies the bundle out of npx's cache and writes a launcher", () => {
  const root = temporary();
  const source = join(root, "npx-cache");
  const data = join(root, "data");
  mkdirSync(source);
  mkdirSync(data);
  writeFileSync(join(source, "monocode-host.mjs"), "host");
  writeFileSync(join(source, "provider-guard.mjs"), "guard");
  const runtime = installRuntime({
    directory: data,
    version: "1.2.3",
    source,
    executable: "/usr/bin/node",
    platform: "linux",
  });
  expect(runtime.entry).toBe(join(data, "runtime", "1.2.3", "monocode-host.mjs"));
  expect(readFileSync(join(data, "runtime", "1.2.3", "provider-guard.mjs"), "utf8")).toBe("guard");
  expect(readFileSync(runtime.launcher, "utf8")).toBe(
    `#!/bin/sh\nexec '/usr/bin/node' '${runtime.entry}' "$@"\n`,
  );
  // Running connect again from the installed copy keeps it in place.
  installRuntime({
    directory: data,
    version: "1.2.3",
    source: join(data, "runtime", "1.2.3"),
    executable: "/usr/bin/node",
    platform: "linux",
  });
  expect(existsSync(runtime.entry)).toBe(true);

  mkdirSync(join(data, "runtime", "0.4.3-linux-x64-.install.abc"));
  writeFileSync(join(data, "runtime-path"), "old");
  pruneRuntimes(data, join(data, "runtime", "1.2.3"));
  expect(existsSync(join(data, "runtime", "0.4.3-linux-x64-.install.abc"))).toBe(false);
  expect(existsSync(join(data, "runtime-path"))).toBe(false);
  expect(existsSync(runtime.entry)).toBe(true);
});

it("refuses an incomplete bundle", () => {
  const root = temporary();
  writeFileSync(join(root, "monocode-host.mjs"), "host");
  expect(() =>
    installRuntime({ directory: join(root, "data"), version: "1.0.0", source: root, executable: "node" }),
  ).toThrow("provider-guard.mjs");
});

it.skipIf(process.platform === "win32")(
  "prefers a PATH link to Node over its versioned install folder",
  () => {
    const root = temporary();
    const cellar = join(root, "Cellar/node/25.0.0/bin");
    const bin = join(root, "bin");
    mkdirSync(cellar, { recursive: true });
    mkdirSync(bin);
    writeFileSync(join(cellar, "node"), "");
    symlinkSync(join(cellar, "node"), join(bin, "node"));
    expect(stableNodePath(join(cellar, "node"), `${cellar}:${bin}`)).toBe(join(bin, "node"));
    expect(stableNodePath(join(cellar, "node"), cellar)).toBe(join(cellar, "node"));
  },
);

it("ranks LAN addresses before overlay networks and skips virtual adapters", () => {
  const entry = (address: string, internal = false) =>
    ({ address, family: "IPv4", internal }) as never;
  expect(
    networkEndpoints(
      3774,
      "0.0.0.0",
      {
        lo0: [entry("127.0.0.1", true)],
        tailscale0: [entry("100.64.0.9")],
        en0: [entry("192.168.1.20"), entry("169.254.3.3")],
        docker0: [entry("172.17.0.1")],
        "vEthernet (WSL)": [entry("172.28.0.1")],
        eth1: [entry("203.0.113.8")],
      },
      ["box.tailnet.ts.net"],
    ),
  ).toEqual([
    "https://192.168.1.20:3774",
    "https://100.64.0.9:3774",
    "https://203.0.113.8:3774",
    "https://box.tailnet.ts.net:3774",
  ]);
  expect(networkEndpoints(3774, "10.0.0.2")).toEqual(["https://10.0.0.2:3774"]);
});

it("round-trips a pairing link", () => {
  const offer = {
    name: "Studio & lab",
    environmentId: "env",
    fingerprint: "f".repeat(43),
    code: "c".repeat(43),
    endpoints: ["https://10.0.0.2:3774", "https://box.ts.net:3774"],
  };
  const link = pairingLink(offer);
  expect(link.startsWith("monocode://pair?v=1&")).toBe(true);
  expect(parsePairingLink(`  ${link}\n`)).toEqual(offer);
  expect(() => parsePairingLink("https://example.com/pair?v=1")).toThrow();
});

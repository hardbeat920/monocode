import { execFile } from "node:child_process";
import { readFileSync, renameSync, writeFileSync } from "node:fs";
import { networkInterfaces } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";

const exec = promisify(execFile);

/** Saved in `<data dir>/network.json`. With network access off, the host
 * listens only on loopback and is reached through an SSH forward. */
export type NetworkSettings = { enabled: boolean; bind: string };
export const DEFAULT_BIND = "0.0.0.0";

export function readNetworkSettings(directory: string): NetworkSettings {
  try {
    const value = JSON.parse(
      readFileSync(join(directory, "network.json"), "utf8"),
    ) as Partial<NetworkSettings>;
    return {
      enabled: value.enabled === true,
      bind:
        typeof value.bind === "string" && value.bind ? value.bind : DEFAULT_BIND,
    };
  } catch {
    return { enabled: false, bind: DEFAULT_BIND };
  }
}

export function writeNetworkSettings(
  directory: string,
  settings: NetworkSettings,
): void {
  const path = join(directory, "network.json");
  writeFileSync(`${path}.tmp`, JSON.stringify(settings), { mode: 0o600 });
  renameSync(`${path}.tmp`, path);
}

// Container bridges, VM switches, and WSL's host-side adapter are not
// reachable from another computer.
const VIRTUAL =
  /^(docker|br-|veth|virbr|vmnet|vboxnet|lxc|lxd|cni|flannel|podman|awdl|llw|bridge|vEthernet \(WSL)/i;

const privateRange = (address: string) =>
  /^10\./.test(address) ||
  /^192\.168\./.test(address) ||
  /^172\.(1[6-9]|2\d|3[01])\./.test(address);
// Tailscale and other overlay networks use the carrier-grade NAT range.
const overlayRange = (address: string) =>
  /^100\.(6[4-9]|[7-9]\d|1[01]\d|12[0-7])\./.test(address);

/**
 * Candidate `https://` addresses for this host, most likely to work first:
 * private LAN addresses, then overlay networks such as Tailscale, then the
 * rest. Desktops try each and keep the one that answers.
 */
export function networkEndpoints(
  port: number,
  bind = DEFAULT_BIND,
  interfaces = networkInterfaces(),
  names: string[] = [],
): string[] {
  const url = (host: string) =>
    `https://${host.includes(":") ? `[${host}]` : host}:${port}`;
  if (bind !== "0.0.0.0" && bind !== "::") return [url(bind)];
  const addresses: string[] = [];
  for (const [name, entries] of Object.entries(interfaces)) {
    if (VIRTUAL.test(name)) continue;
    for (const entry of entries ?? []) {
      if (entry.internal || entry.family !== "IPv4") continue;
      if (entry.address.startsWith("169.254.")) continue;
      addresses.push(entry.address);
    }
  }
  const rank = (address: string) =>
    privateRange(address) ? 0 : overlayRange(address) ? 1 : 2;
  const ordered = [...new Set(addresses)].sort((a, b) => rank(a) - rank(b));
  return [...ordered, ...names].map(url);
}

/** This machine's Tailscale MagicDNS name, when Tailscale is running. */
export async function tailscaleName(): Promise<string | undefined> {
  const candidates =
    process.platform === "darwin"
      ? ["tailscale", "/Applications/Tailscale.app/Contents/MacOS/Tailscale"]
      : process.platform === "win32"
        ? ["tailscale.exe", "C:\\Program Files\\Tailscale\\tailscale.exe"]
        : ["tailscale"];
  for (const command of candidates) {
    try {
      const { stdout } = await exec(command, ["status", "--json", "--peers=false"], {
        timeout: 3_000,
        maxBuffer: 4 * 1024 * 1024,
        windowsHide: true,
      });
      const self = (JSON.parse(stdout) as { Self?: { DNSName?: string; Online?: boolean } })
        .Self;
      const name = self?.DNSName?.replace(/\.$/, "");
      return name && /^[a-z0-9.-]+$/i.test(name) ? name : undefined;
    } catch {
      /* not installed, or not running */
    }
  }
}

export type PairingOffer = {
  name: string;
  environmentId: string;
  fingerprint: string;
  code: string;
  endpoints: string[];
};

/** A link the desktop accepts in Settings → Connections → Pair machine. The
 * code is single-use; the fingerprint pins the host's TLS certificate. */
export function pairingLink(offer: PairingOffer): string {
  const query = new URLSearchParams({
    v: "1",
    name: offer.name,
    id: offer.environmentId,
    fp: offer.fingerprint,
    code: offer.code,
  });
  for (const endpoint of offer.endpoints) query.append("url", endpoint);
  return `monocode://pair?${query}`;
}

export function parsePairingLink(link: string): PairingOffer {
  const url = new URL(link.trim());
  const query = url.searchParams;
  if (url.protocol !== "monocode:" || url.hostname !== "pair" || query.get("v") !== "1")
    throw new Error("Not a MonoCode pairing link");
  return {
    name: query.get("name") ?? "",
    environmentId: query.get("id") ?? "",
    fingerprint: query.get("fp") ?? "",
    code: query.get("code") ?? "",
    endpoints: query.getAll("url"),
  };
}

import { useEffect, useState } from "react";
import { version as desktopVersion } from "../../../../package.json";
import type { HostDescriptor } from "./protocol";

// Require a complete SemVer; unknown or malformed versions must not suggest a
// downgrade. Accept the conventional v prefix and ignore build metadata.
function parseVersion(value: string) {
  const match = value
    .trim()
    .match(
      /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/,
    );
  if (!match) return;
  const prerelease = match[4]?.split(".") ?? [];
  if (prerelease.some((part) => /^\d+$/.test(part) && /^0\d/.test(part)))
    return;
  return { core: match.slice(1, 4).map(BigInt), prerelease };
}

export function isOlderHostVersion(
  hostVersion: string | undefined,
  clientVersion = desktopVersion,
): boolean {
  if (typeof hostVersion !== "string") return false;
  const host = parseVersion(hostVersion);
  const client = parseVersion(clientVersion);
  if (!host || !client) return false;
  for (let i = 0; i < 3; i++) {
    if (host.core[i] !== client.core[i]) return host.core[i] < client.core[i];
  }
  if (!host.prerelease.length || !client.prerelease.length)
    return !!host.prerelease.length && !client.prerelease.length;
  for (
    let i = 0;
    i < Math.max(host.prerelease.length, client.prerelease.length);
    i++
  ) {
    const a = host.prerelease[i];
    const b = client.prerelease[i];
    if (a === b) continue;
    if (a === undefined) return true;
    if (b === undefined) return false;
    const aNumeric = /^\d+$/.test(a);
    const bNumeric = /^\d+$/.test(b);
    if (aNumeric && bNumeric) return BigInt(a) < BigInt(b);
    if (aNumeric !== bNumeric) return aNumeric;
    return a < b;
  }
  return false;
}

export function hostNeedsCapabilities(host: HostDescriptor): boolean {
  return (
    !host.capabilities?.includes("workspace.run") ||
    !host.capabilities?.includes("git.worktreeCreate")
  );
}

export function hostUpdateAvailable(host: HostDescriptor): boolean {
  return isOlderHostVersion(host.hostVersion) || hostNeedsCapabilities(host);
}

const CHANGE = "monocode:host-update";
const updates = new Map<string, boolean>();

/** Share descriptor checks from Settings, sessions and connection heartbeats. */
export function recordHostUpdate(machineId: string, host: HostDescriptor) {
  const available = hostUpdateAvailable(host);
  if (updates.get(machineId) === available) return;
  updates.set(machineId, available);
  window.dispatchEvent(new Event(CHANGE));
}

export function useHostUpdateAvailable(machineId?: string): boolean {
  const read = () => !!machineId && updates.get(machineId) === true;
  const [available, setAvailable] = useState(read);
  useEffect(() => {
    const changed = () => setAvailable(read());
    changed();
    window.addEventListener(CHANGE, changed);
    return () => window.removeEventListener(CHANGE, changed);
  }, [machineId]);
  return available;
}

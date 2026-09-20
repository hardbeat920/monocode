import { useSyncExternalStore } from "react";

import type { ConnectionProfile } from "./connections";
import { listConnections } from "./connections";

/**
 * Remote projects are identified by URI-shaped cwd strings:
 * `ssh://<connectionId>/<remote-absolute-path>`. Downstream systems
 * (project keys, recents, the session store) treat the string as opaque;
 * these helpers are the only place that interprets the shape.
 */

const REMOTE_PREFIX = "ssh://";

export function isRemotePath(path: string): boolean {
  return path.startsWith(REMOTE_PREFIX);
}

export type ParsedRemotePath = {
  connectionId: string;
  /** Remote absolute path, always starts with "/". */
  path: string;
};

export function parseRemotePath(path: string): ParsedRemotePath | null {
  if (!isRemotePath(path)) return null;
  const rest = path.slice(REMOTE_PREFIX.length);
  const slash = rest.indexOf("/");
  if (slash <= 0) return null;
  return { connectionId: rest.slice(0, slash), path: rest.slice(slash) };
}

export function remoteProjectUri(connectionId: string, path: string): string {
  const remotePath = path.startsWith("/") ? path : `/${path}`;
  return `${REMOTE_PREFIX}${connectionId}${remotePath}`;
}

// ---------------------------------------------------------------------------
// Connection profile cache.
//
// `prettyCwd` and other display helpers stay synchronous, so profiles are
// mirrored into a module-level store that React subscribes to via
// useSyncExternalStore. The Rust store remains the source of truth.
// ---------------------------------------------------------------------------

let profiles: ConnectionProfile[] = [];
const listeners = new Set<() => void>();

function notify() {
  listeners.forEach((listener) => listener());
}

export function hydrateConnections(next: ConnectionProfile[]) {
  profiles = Array.isArray(next) ? next : [];
  notify();
}

export async function refreshConnections(): Promise<ConnectionProfile[]> {
  const next = await listConnections();
  const safe = Array.isArray(next) ? next : [];
  hydrateConnections(safe);
  return safe;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useConnections(): ConnectionProfile[] {
  useSyncExternalStore(
    subscribe,
    () => profiles,
    () => profiles,
  );
  return profiles;
}

export function connectionById(id: string): ConnectionProfile | null {
  return profiles.find((profile) => profile.id === id) ?? null;
}

/** Sync display label for a remote path: the connection's name, else its id. */
export function remoteConnectionLabel(path: string): string {
  const parsed = parseRemotePath(path);
  if (!parsed) return path;
  return connectionById(parsed.connectionId)?.name ?? parsed.connectionId;
}

import { invoke } from "@tauri-apps/api/core";

/**
 * SSH connection profiles — the servers (and server+container targets)
 * remote projects point at. Managed in Settings › Servers; the Rust store in
 * app data (`connections.json`) is the source of truth.
 */

export type ConnectionProfile = {
  /** Flat charset id embedded in `ssh://` URIs. */
  id: string;
  name: string;
  /** Host, IP, or a ~/.ssh/config alias — the system ssh resolves it. */
  host: string;
  user: string | null;
  port: number | null;
  /** When set, sessions run inside this Docker container on the host. */
  container: string | null;
  authNote: string | null;
  createdAt: number;
};

export type ConnectionTestResult = {
  ok: boolean;
  latencyMs: number;
  error: string | null;
  /** Remote $HOME — seeds the path browser when connecting. */
  home: string | null;
  uname: string | null;
};

export function listConnections(): Promise<ConnectionProfile[]> {
  return invoke<ConnectionProfile[]>("connections_list");
}

/**
 * Validate by connecting, then persist — a typo'd host never saves. Throws
 * the connection error text on failure.
 */
export function saveConnection(
  profile: ConnectionProfile,
): Promise<ConnectionProfile> {
  return invoke<ConnectionProfile>("connections_save", { profile });
}

export function removeConnection(id: string): Promise<void> {
  return invoke<void>("connections_remove", { id });
}

/** Probe without persisting anything. */
export function testConnection(
  profile: ConnectionProfile,
): Promise<ConnectionTestResult> {
  return invoke<ConnectionTestResult>("connections_test", { profile });
}

/** Remote $HOME for a connection — seeds the connect dialog's path browser. */
export function remoteHome(connectionId: string): Promise<string> {
  return invoke<string>("remote_home", { connectionId });
}

export type RemoteBinary = { path: string };

/**
 * Resolve an agent CLI on a remote target (login shell + TTL cache). Throws
 * with an install hint when the CLI is missing there.
 */
export function resolveRemoteAgent(
  connectionId: string,
  agent: string,
): Promise<RemoteBinary> {
  return invoke<RemoteBinary>("remote_resolve_agent", { connectionId, agent });
}

/** One-shot captured exec of a resolved remote agent binary. */
export function remoteExecChild(
  connectionId: string,
  command: string,
  args: string[],
): Promise<string> {
  return invoke<string>("remote_harness_exec", { connectionId, command, args });
}

/** New-profile template for forms. */
export function blankConnectionProfile(): ConnectionProfile {
  return {
    id: "",
    name: "",
    host: "",
    user: null,
    port: null,
    container: null,
    authNote: null,
    createdAt: 0,
  };
}

/** Canned install commands for the agent CLIs remote sessions support. */
export const REMOTE_AGENT_INSTALLS: { agent: string; command: string }[] = [
  { agent: "Claude Code", command: "npm install -g @anthropic-ai/claude-code" },
  { agent: "Codex", command: "npm install -g @openai/codex" },
];

export type RemoteContainer = { id: string; name: string; image: string; status: string };
export type RemoteDirectoryListing = { path: string; home: string; directories: string[] };

export function sshConfigHosts(): Promise<string[]> {
  return invoke<string[]>("connections_ssh_hosts");
}
export function listRemoteContainers(profile: ConnectionProfile): Promise<RemoteContainer[]> {
  return invoke("connections_containers", { profile });
}
export function browseRemoteDirectories(profile: ConnectionProfile, path: string): Promise<RemoteDirectoryListing> {
  return invoke("connections_browse", { profile, path });
}

export function sameServer(a: ConnectionProfile, b: ConnectionProfile): boolean {
  return a.host === b.host && a.user === b.user && a.port === b.port;
}

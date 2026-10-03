import { selectedProviderAccountId } from "../../providers/model/providerAccounts";
import { invoke } from "@tauri-apps/api/core";
import { parseClaudeMcpList, type McpConnection } from "./mcp";

export type McpServerRow = McpConnection & { status: string };

export type McpSettingsSnapshot = {
  servers: McpServerRow[];
  error: string;
  claudeError: string;
};

const snapshots = new Map<string, McpSettingsSnapshot>();
const requests = new Map<string, Promise<McpSettingsSnapshot>>();
const healthRequests = new Map<string, Promise<void>>();
const listeners = new Map<
  string,
  Set<(snapshot: McpSettingsSnapshot) => void>
>();

function scopeFor(
  cwd: string,
  accountId = selectedProviderAccountId("claude", cwd),
) {
  return {
    key: `${cwd}\0${accountId}`,
    args: { cwd, ...(accountId === "default" ? {} : { accountId }) },
  };
}

export function getCachedMcpSettings(cwd: string, accountId?: string) {
  return snapshots.get(scopeFor(cwd, accountId).key);
}

export function subscribeMcpSettings(
  cwd: string,
  listener: (snapshot: McpSettingsSnapshot) => void,
  accountId?: string,
) {
  const key = scopeFor(cwd, accountId).key;
  const subscribers = listeners.get(key) ?? new Set();
  subscribers.add(listener);
  listeners.set(key, subscribers);
  return () => {
    subscribers.delete(listener);
    if (subscribers.size === 0) listeners.delete(key);
  };
}

function publish(key: string, snapshot: McpSettingsSnapshot) {
  snapshots.set(key, snapshot);
  listeners.get(key)?.forEach((listener) => listener(snapshot));
}

/** Discovery is shared across settings and pickers; health never delays the list. */
export function loadMcpSettings(
  cwd: string,
  force = false,
  options: { claudeHealth?: boolean; accountId?: string } = {},
) {
  const { key, args } = scopeFor(cwd, options.accountId);
  let request = requests.get(key);
  if (!request || force) {
    const discovery = fetchMcpSettings(args).then((snapshot) => {
      if (requests.get(key) === discovery) {
        healthRequests.delete(key);
        publish(key, snapshot);
      }
      return snapshot;
    });
    requests.set(key, discovery);
    request = discovery;
  }
  const discovery = request;
  return discovery.then((snapshot) => {
    if (
      options.claudeHealth !== false &&
      !snapshot.error &&
      requests.get(key) === discovery
    ) {
      loadClaudeHealth(key, args, discovery);
    }
    return snapshots.get(key) ?? snapshot;
  });
}

async function fetchMcpSettings(args: {
  cwd: string;
  accountId?: string;
}): Promise<McpSettingsSnapshot> {
  try {
    const configured = await invoke<McpConnection[]>("mcp_discover", args);
    const servers = configured.map((server) => ({
      ...server,
      status: server.enabled === false ? "Disabled" : "Configured",
    }));
    return { servers, error: "", claudeError: "" };
  } catch (cause) {
    return { servers: [], error: String(cause), claudeError: "" };
  }
}

function loadClaudeHealth(
  key: string,
  args: { cwd: string; accountId?: string },
  discovery: Promise<McpSettingsSnapshot>,
) {
  if (healthRequests.has(key)) return;
  const request = invoke<string>("claude_mcp_list", args)
    .then((output) => {
      if (requests.get(key) !== discovery) return;
      const snapshot = snapshots.get(key)!;
      const health = new Map(
        parseClaudeMcpList(output).map((server) => [
          server.name,
          server.status,
        ]),
      );
      const servers: McpServerRow[] = snapshot.servers.map((server) => ({
        ...server,
        status:
          server.enabled === false
            ? "Disabled"
            : server.provider === "claude"
              ? (health.get(server.name) ?? "Configured")
              : server.status,
      }));
      // Claude can supply connections that are not stored in a local config file.
      for (const [name, status] of health) {
        if (
          servers.some(
            (server) => server.provider === "claude" && server.name === name,
          )
        )
          continue;
        servers.push({
          provider: "claude",
          name,
          scope: "local",
          configPath: "",
          transport: "",
          status,
        });
      }
      publish(key, { ...snapshot, servers, claudeError: "" });
    })
    .catch((cause) => {
      if (requests.get(key) !== discovery) return;
      publish(key, { ...snapshots.get(key)!, claudeError: String(cause) });
    });
  healthRequests.set(key, request);
}

export function clearMcpSettingsCache() {
  snapshots.clear();
  requests.clear();
  healthRequests.clear();
}

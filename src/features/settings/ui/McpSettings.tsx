import { invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";
import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type FormEvent,
} from "react";
import { revealPath } from "../../../platform/tauri/fs";
import {
  MCP_PROVIDER_LABELS,
  parseClaudeMcpList,
  type McpConnection,
} from "../model/mcp";

type Scope = McpConnection["scope"];
type ServerRow = McpConnection & { status: string };
type Filter = "all" | McpConnection["provider"];

export function McpSettings({ cwd }: { cwd: string }) {
  const [servers, setServers] = useState<ServerRow[]>([]);
  const [filter, setFilter] = useState<Filter>("all");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [claudeError, setClaudeError] = useState("");
  const [name, setName] = useState("");
  const [config, setConfig] = useState("");
  const [scope, setScope] = useState<Scope>("local");
  const [removeScopes, setRemoveScopes] = useState<Record<string, Scope>>({});

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const configured = await invoke<McpConnection[]>("mcp_discover", { cwd });
      let health = new Map<string, string>();
      try {
        const output = await invoke<string>("claude_mcp_list", { cwd });
        health = new Map(
          parseClaudeMcpList(output).map((server) => [
            server.name,
            server.status,
          ]),
        );
        setClaudeError("");
      } catch (cause) {
        setClaudeError(String(cause));
      }
      const rows: ServerRow[] = configured.map((server) => ({
        ...server,
        status:
          server.provider === "claude"
            ? (health.get(server.name) ?? "Configured")
            : "Configured",
      }));
      // Claude can supply connections that are not stored in a local config file.
      for (const [serverName, status] of health) {
        if (
          rows.some(
            (row) => row.provider === "claude" && row.name === serverName,
          )
        )
          continue;
        rows.push({
          provider: "claude",
          name: serverName,
          scope: "local",
          configPath: "",
          transport: "",
          status,
        });
      }
      setServers(rows);
      setError("");
    } catch (cause) {
      setServers([]);
      setError(String(cause));
    } finally {
      setLoading(false);
    }
  }, [cwd]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const visible = useMemo(
    () =>
      filter === "all"
        ? servers
        : servers.filter((server) => server.provider === filter),
    [filter, servers],
  );

  async function add(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      const parsed: unknown = JSON.parse(config);
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
        throw new Error("Configuration must be a JSON object");
      }
      setBusy("add");
      setError("");
      await invoke("claude_mcp_add", { cwd, name: name.trim(), config, scope });
      setName("");
      setConfig("");
      await refresh();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(null);
    }
  }

  async function login(server: ServerRow) {
    setBusy(server.name);
    setError("");
    try {
      await invoke("mcp_provider_login", {
        cwd,
        provider: server.provider,
        name: server.name,
      });
      await refresh();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(null);
    }
  }

  async function remove(server: ServerRow) {
    const selectedScope = server.configPath
      ? server.scope
      : (removeScopes[server.name] ?? "local");
    if (
      !(await ask(`Remove ${server.name} from ${selectedScope} scope?`, {
        title: "Remove MCP server",
      }))
    )
      return;
    setBusy(server.name);
    setError("");
    try {
      await invoke("claude_mcp_remove", {
        cwd,
        name: server.name,
        scope: selectedScope,
      });
      await refresh();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(null);
    }
  }

  return (
    <div
      id="setting-mcp-servers"
      data-setting-id="mcp-servers"
      className="space-y-6"
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-sm font-semibold">MCP connections</h2>
          <p className="mt-1 text-xs text-content/55">
            Configured servers for this project and your provider accounts.
          </p>
        </div>
        <button
          type="button"
          onClick={() => void refresh()}
          disabled={loading || busy !== null}
          className="rounded-md border border-stroke px-3 py-1.5 text-xs hover:bg-content/5 disabled:opacity-50"
        >
          Refresh
        </button>
      </div>
      <div
        className="flex flex-wrap gap-1"
        aria-label="Filter MCP servers by provider"
      >
        {(["all", "claude", "codex", "cursor", "opencode"] as const).map(
          (provider) => (
            <button
              key={provider}
              type="button"
              aria-pressed={filter === provider}
              onClick={() => setFilter(provider)}
              className={`rounded-md px-2.5 py-1 text-xs ${filter === provider ? "bg-selection text-content" : "text-content/55 hover:bg-content/5 hover:text-content"}`}
            >
              {provider === "all" ? "All" : MCP_PROVIDER_LABELS[provider]}
              <span className="ml-1 opacity-60">
                {provider === "all"
                  ? servers.length
                  : servers.filter((server) => server.provider === provider)
                      .length}
              </span>
            </button>
          ),
        )}
      </div>
      {error ? (
        <p
          role="alert"
          className="rounded-md border border-red-500/30 bg-red-500/10 p-3 text-xs text-red-400"
        >
          {error}
        </p>
      ) : null}
      {claudeError && (filter === "all" || filter === "claude") ? (
        <p className="text-xs text-content/55">
          Claude connection status unavailable: {claudeError}
        </p>
      ) : null}
      {loading ? (
        <p className="text-sm text-content/55">Checking servers…</p>
      ) : visible.length === 0 ? (
        <p className="text-sm text-content/55">
          No MCP servers configured for this provider.
        </p>
      ) : (
        <div className="divide-y divide-stroke rounded-lg border border-stroke">
          {visible.map((server) => (
            <div
              key={`${server.provider}:${server.scope}:${server.configPath}:${server.name}`}
              className="flex flex-wrap items-center gap-3 p-3"
            >
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium">{server.name}</div>
                <div className="text-xs text-content/55">
                  {MCP_PROVIDER_LABELS[server.provider]} · {server.scope} ·{" "}
                  {server.transport || "MCP"} · {server.status}
                </div>
                {server.configPath ? (
                  <div
                    className="truncate text-[11px] text-content/40"
                    title={server.configPath}
                  >
                    {server.configPath}
                  </div>
                ) : null}
              </div>
              {!["stdio", "local", "ws"].includes(server.transport) ? (
                <button
                  type="button"
                  disabled={busy !== null}
                  onClick={() => void login(server)}
                  className="rounded-md border border-stroke px-2 py-1 text-xs hover:bg-content/5 disabled:opacity-50"
                >
                  Sign in
                </button>
              ) : null}
              {server.provider === "claude" ? (
                <>
                  {!server.configPath ? (
                    <label className="text-xs text-content/55">
                      Scope{" "}
                      <select
                        aria-label={`Scope to remove ${server.name} from`}
                        value={removeScopes[server.name] ?? "local"}
                        onChange={(event) =>
                          setRemoveScopes((current) => ({
                            ...current,
                            [server.name]: event.target.value as Scope,
                          }))
                        }
                        className="rounded border border-stroke bg-background-base px-1 py-1 text-content"
                      >
                        <option value="local">Local</option>
                        <option value="project">Project</option>
                        <option value="user">User</option>
                      </select>
                    </label>
                  ) : null}
                  <button
                    type="button"
                    disabled={busy !== null}
                    onClick={() => void remove(server)}
                    className="rounded-md border border-stroke px-2 py-1 text-xs hover:bg-content/5 disabled:opacity-50"
                  >
                    Remove
                  </button>
                </>
              ) : (
                <button
                  type="button"
                  onClick={() =>
                    void revealPath(server.configPath).catch((cause) =>
                      setError(String(cause)),
                    )
                  }
                  className="rounded-md border border-stroke px-2 py-1 text-xs hover:bg-content/5"
                >
                  Show config
                </button>
              )}
            </div>
          ))}
        </div>
      )}
      <form
        onSubmit={(event) => void add(event)}
        className="space-y-3 rounded-lg border border-stroke p-4"
      >
        <div>
          <h2 className="text-sm font-semibold">Add a Claude Code server</h2>
          <p className="mt-1 text-xs text-content/55">
            Paste a Claude Code MCP server JSON object. Other providers keep
            their own configuration files, shown above.
          </p>
        </div>
        <label className="block text-xs text-content/65">
          Name
          <input
            required
            pattern="[A-Za-z0-9_-]+"
            value={name}
            onChange={(event) => setName(event.target.value)}
            className="mt-1 block w-full rounded-md border border-stroke bg-background-base px-2 py-1.5 text-sm text-content"
            placeholder="my-server"
          />
        </label>
        <label className="block text-xs text-content/65">
          Configuration
          <textarea
            required
            value={config}
            onChange={(event) => setConfig(event.target.value)}
            rows={4}
            spellCheck={false}
            className="mt-1 block w-full rounded-md border border-stroke bg-background-base px-2 py-1.5 font-mono text-xs text-content"
            placeholder={'{"type":"http","url":"https://example.com/mcp"}'}
          />
        </label>
        <div className="flex items-center gap-3">
          <label className="text-xs text-content/65">
            Scope{" "}
            <select
              value={scope}
              onChange={(event) => setScope(event.target.value as Scope)}
              className="rounded border border-stroke bg-background-base px-2 py-1 text-content"
            >
              <option value="local">Local</option>
              <option value="project">Project</option>
              <option value="user">User</option>
            </select>
          </label>
          <button
            type="submit"
            disabled={busy !== null}
            className="rounded-md border border-stroke px-3 py-1.5 text-xs hover:bg-content/5 disabled:opacity-50"
          >
            Add server
          </button>
        </div>
      </form>
      <p className="text-xs text-content/45">
        Claude status comes from its CLI. Other providers show configured
        entries; open their config to manage them. Claude OAuth sign in opens
        your browser when supported.
      </p>
    </div>
  );
}

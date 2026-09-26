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
import { HarnessIcon } from "../../sessions/ui/HarnessIcon";
import { Modal } from "../../../shared/ui/Modal";
import { Globe, Plus, RefreshCw, ChevronDown } from "../../../shared/ui/icons";
import {
  MCP_PROVIDER_LABELS,
  parseClaudeMcpList,
  type McpConnection,
} from "../model/mcp";

type Scope = McpConnection["scope"];
type ServerRow = McpConnection & { status: string };
type Filter = "all" | McpConnection["provider"];
type Provider = McpConnection["provider"];
const PROVIDERS: Provider[] = [
  "claude",
  "claude_desktop",
  "codex",
  "cursor",
  "opencode",
];
const SCOPES: Record<Provider, Scope[]> = {
  claude: ["local", "project", "user"],
  claude_desktop: ["user"],
  codex: ["user"],
  cursor: ["project", "user"],
  opencode: ["project", "user"],
};

function ProviderIcon({ provider }: { provider: Provider }) {
  return (
    <HarnessIcon
      harness={provider === "claude_desktop" ? "claude" : provider}
      className="size-3.5 shrink-0"
    />
  );
}

function AddServerModal({
  cwd,
  initialProvider,
  onClose,
  onAdded,
}: {
  cwd: string;
  initialProvider: Provider;
  onClose: () => void;
  onAdded: () => Promise<void>;
}) {
  const [provider, setProvider] = useState<Provider>(initialProvider);
  const [scope, setScope] = useState<Scope>(SCOPES[initialProvider][0]);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [name, setName] = useState("");
  const [config, setConfig] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  async function add(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      const parsed: unknown = JSON.parse(config);
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
        throw new Error("Configuration must be a JSON object");
      }
      setBusy(true);
      setError("");
      await invoke("mcp_add", {
        cwd,
        provider,
        scope,
        name: name.trim(),
        config,
      });
      await onAdded();
      onClose();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      title="Add MCP server"
      description="Paste a server configuration and choose where to add it."
      onClose={onClose}
      fitViewport
    >
      <form onSubmit={(event) => void add(event)} className="space-y-4 p-4">
        <div className="grid gap-3 sm:grid-cols-2">
          <div className="relative text-xs text-content/65">
            <span>Provider</span>
            <button
              type="button"
              aria-label="Provider"
              aria-expanded={pickerOpen}
              onClick={() => setPickerOpen(!pickerOpen)}
              className="mt-1 flex w-full items-center gap-2 rounded-md border border-stroke bg-background-base px-2 py-1.5 text-left text-sm text-content"
            >
              <ProviderIcon provider={provider} />
              <span className="flex-1">{MCP_PROVIDER_LABELS[provider]}</span>
              <ChevronDown className="size-3.5" />
            </button>
            {pickerOpen ? (
              <div
                role="listbox"
                aria-label="Choose provider"
                className="absolute z-10 mt-1 w-full rounded-md border border-stroke bg-background-base p-1 shadow-lg"
              >
                {PROVIDERS.map((option) => (
                  <button
                    key={option}
                    type="button"
                    role="option"
                    aria-selected={provider === option}
                    onClick={() => {
                      setProvider(option);
                      setScope(SCOPES[option][0]);
                      setPickerOpen(false);
                    }}
                    className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm text-content hover:bg-content/5"
                  >
                    <ProviderIcon provider={option} />
                    {MCP_PROVIDER_LABELS[option]}
                  </button>
                ))}
              </div>
            ) : null}
          </div>
          <label className="text-xs text-content/65">
            Scope
            <select
              value={scope}
              onChange={(event) => setScope(event.target.value as Scope)}
              className="mt-1 block w-full rounded-md border border-stroke bg-background-base px-2 py-1.5 text-sm text-content"
            >
              {SCOPES[provider].map((option) => (
                <option key={option} value={option}>
                  {option[0].toUpperCase() + option.slice(1)}
                </option>
              ))}
            </select>
          </label>
        </div>
        <label className="block text-xs text-content/65">
          Name{" "}
          <span className="text-content/40">
            (optional for an mcpServers block)
          </span>
          <input
            value={name}
            pattern="[A-Za-z0-9_-]*"
            onChange={(event) => setName(event.target.value)}
            className="mt-1 block w-full rounded-md border border-stroke bg-background-base px-2 py-1.5 text-sm text-content"
            placeholder="my-server"
          />
        </label>
        <label className="block text-xs text-content/65">
          JSON configuration
          <textarea
            required
            value={config}
            onChange={(event) => setConfig(event.target.value)}
            rows={7}
            spellCheck={false}
            className="mt-1 block w-full rounded-md border border-stroke bg-background-base px-2 py-1.5 font-mono text-xs text-content"
            placeholder={
              '{"mcpServers":{"my-server":{"command":"npx","args":["-y","example-mcp"]}}}'
            }
          />
        </label>
        <p className="text-xs text-content/45">
          Paste one entry from an mcpServers block, or a single server object
          with a name above.
        </p>
        {error ? (
          <p
            role="alert"
            className="rounded-md border border-red-500/30 bg-red-500/10 p-2 text-xs text-red-400"
          >
            {error}
          </p>
        ) : null}
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md border border-stroke px-3 py-1.5 text-xs hover:bg-content/5"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={busy}
            className="rounded-md border border-stroke px-3 py-1.5 text-xs hover:bg-content/5 disabled:opacity-50"
          >
            {busy ? "Adding…" : "Add server"}
          </button>
        </div>
      </form>
    </Modal>
  );
}

export function McpSettings({ cwd }: { cwd: string }) {
  const [servers, setServers] = useState<ServerRow[]>([]);
  const [filter, setFilter] = useState<Filter>("all");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [claudeError, setClaudeError] = useState("");
  const [addOpen, setAddOpen] = useState(false);
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
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => void refresh()}
            disabled={loading || busy !== null}
            className="flex items-center gap-1.5 rounded-md border border-stroke px-3 py-1.5 text-xs hover:bg-content/5 disabled:opacity-50"
          >
            <RefreshCw className="size-3.5" />
            Refresh
          </button>
          <button
            type="button"
            aria-label="Add MCP server"
            onClick={() => setAddOpen(true)}
            className="grid size-7 place-items-center rounded-md border border-stroke hover:bg-content/5"
          >
            <Plus className="size-3.5" />
          </button>
        </div>
      </div>
      <div
        className="flex flex-wrap gap-1"
        aria-label="Filter MCP servers by provider"
      >
        {(["all", ...PROVIDERS] as const).map((provider) => (
          <button
            key={provider}
            type="button"
            aria-pressed={filter === provider}
            onClick={() => setFilter(provider)}
            className={`inline-flex items-center gap-1.5 rounded-md px-2.5 py-1 text-xs ${filter === provider ? "bg-selection text-content" : "text-content/55 hover:bg-content/5 hover:text-content"}`}
          >
            {provider === "all" ? (
              <Globe className="size-3.5" />
            ) : (
              <ProviderIcon provider={provider} />
            )}
            {provider === "all" ? "All" : MCP_PROVIDER_LABELS[provider]}
            <span className="opacity-60">
              {provider === "all"
                ? servers.length
                : servers.filter((server) => server.provider === provider)
                    .length}
            </span>
          </button>
        ))}
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
              {server.provider !== "claude_desktop" &&
              !["stdio", "local", "ws"].includes(server.transport) ? (
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
      <p className="text-xs text-content/45">
        Claude Code status comes from its CLI. Other providers show configured
        entries. Sign in opens your browser when supported.
      </p>
      {addOpen ? (
        <AddServerModal
          cwd={cwd}
          initialProvider={filter === "all" ? "claude" : filter}
          onClose={() => setAddOpen(false)}
          onAdded={refresh}
        />
      ) : null}
    </div>
  );
}

import { invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type ReactNode,
} from "react";
import { revealPath } from "../../../platform/tauri/fs";
import { HarnessIcon } from "../../sessions/ui/HarnessIcon";
import { SearchableProjectPicker } from "../../projects/ui/SearchableProjectPicker";
import type { RecentProject } from "../../projects/model/recents";
import { Modal } from "../../../shared/ui/Modal";
import { Popover } from "../../../shared/ui/Popover";
import { LAYER } from "../../../shared/lib/layers";
import {
  Globe,
  Plus,
  RefreshCw,
  ChevronDown,
  ListFilter,
  Check,
} from "../../../shared/ui/icons";
import { MCP_PROVIDER_LABELS, type McpConnection } from "../model/mcp";
import {
  getCachedMcpSettings,
  loadMcpSettings,
  subscribeMcpSettings,
  type McpServerRow,
  type McpSettingsSnapshot,
} from "../model/mcpSettingsCache";

type Scope = McpConnection["scope"];
type ServerRow = McpServerRow;
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

function McpPicker<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: { value: T; label: string; icon?: ReactNode }[];
  onChange: (value: T) => void;
}) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const selected = options.find((option) => option.value === value);
  return (
    <div ref={anchor} className="relative min-w-0">
      <span className="text-xs text-content/65">{label}</span>
      <button
        ref={trigger}
        type="button"
        aria-label={`${label}: ${selected?.label ?? value}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        className="mt-1 flex h-8 w-full items-center gap-2 rounded-md border border-content/10 bg-content/5 px-2 text-left text-[12px] text-content outline-none hover:border-content/20 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent"
      >
        {selected?.icon}
        <span className="min-w-0 flex-1 truncate">
          {selected?.label ?? value}
        </span>
        <ChevronDown
          className={`size-3.5 shrink-0 text-content/50 transition-transform ${open ? "rotate-180" : ""}`}
          strokeWidth={1.75}
        />
      </button>
      {open ? (
        <Popover
          anchor={anchor}
          side="bottom"
          align="start"
          width={240}
          maxHeight={320}
          layer={LAYER.dialogPopover}
          autoFocus
          onDismiss={() => setOpen(false)}
          role="listbox"
          aria-label={label}
          data-dialog-popover
          onKeyDown={(event) => {
            if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
            event.preventDefault();
            const choices = Array.from(
              event.currentTarget.querySelectorAll<HTMLButtonElement>(
                '[role="option"]',
              ),
            );
            if (choices.length === 0) return;
            const current = choices.indexOf(
              document.activeElement as HTMLButtonElement,
            );
            const direction = event.key === "ArrowDown" ? 1 : -1;
            const next =
              current < 0
                ? direction === 1
                  ? 0
                  : choices.length - 1
                : (current + direction + choices.length) % choices.length;
            choices[next].focus();
          }}
          className="overflow-y-auto overscroll-contain p-1"
        >
          {options.map((option) => (
            <button
              key={option.value}
              type="button"
              role="option"
              aria-selected={value === option.value}
              onClick={() => {
                onChange(option.value);
                setOpen(false);
                trigger.current?.focus();
              }}
              className={`flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-[12px] focus-visible:outline-2 focus-visible:outline-accent ${value === option.value ? "bg-selection text-content" : "text-content hover:bg-content/5"}`}
            >
              {option.icon}
              <span className="min-w-0 flex-1 truncate">{option.label}</span>
              {value === option.value ? (
                <Check className="size-3.5 shrink-0" />
              ) : null}
            </button>
          ))}
        </Popover>
      ) : null}
    </div>
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
          <McpPicker
            label="Provider"
            value={provider}
            options={PROVIDERS.map((option) => ({
              value: option,
              label: MCP_PROVIDER_LABELS[option],
              icon: <ProviderIcon provider={option} />,
            }))}
            onChange={(next) => {
              setProvider(next);
              setScope(SCOPES[next][0]);
            }}
          />
          <McpPicker
            label="Scope"
            value={scope}
            options={SCOPES[provider].map((option) => ({
              value: option,
              label: option[0].toUpperCase() + option.slice(1),
            }))}
            onChange={setScope}
          />
        </div>
        <label className="block text-xs text-content/65">
          Name{" "}
          <span className="text-content/40">
            (optional for an mcpServers block)
          </span>
          <input
            value={name}
            pattern={provider === "opencode" ? undefined : "[A-Za-z0-9_-]*"}
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

export function McpSettings({
  cwd,
  recents = [],
}: {
  cwd: string;
  recents?: RecentProject[];
}) {
  const [selection, setSelection] = useState({ initialCwd: cwd, project: cwd });
  const project = selection.initialCwd === cwd ? selection.project : cwd;
  return (
    <McpConnections
      key={project}
      cwd={project}
      projectPicker={
        <SearchableProjectPicker
          cwd={project}
          railCwd={cwd}
          recents={recents}
          className="w-fit max-w-full shrink-0"
          buttonClassName="h-7.5 max-w-full gap-2 bg-content/5 px-2.5 text-[13px] hover:bg-content/12 active:scale-[0.98]"
          onSelectProject={(path) =>
            setSelection({ initialCwd: cwd, project: path })
          }
        />
      }
    />
  );
}

function McpConnections({
  cwd,
  projectPicker,
}: {
  cwd: string;
  projectPicker: ReactNode;
}) {
  const cached = getCachedMcpSettings(cwd);
  const [servers, setServers] = useState<ServerRow[]>(cached?.servers ?? []);
  const [filter, setFilter] = useState<Filter>("all");
  const [showAllProviders, setShowAllProviders] = useState(false);
  const [loading, setLoading] = useState(!cached);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState(cached?.error ?? "");
  const [claudeError, setClaudeError] = useState(cached?.claudeError ?? "");
  const [addOpen, setAddOpen] = useState(false);
  const [removeScopes, setRemoveScopes] = useState<Record<string, Scope>>({});
  const refreshGeneration = useRef(0);

  const applySnapshot = useCallback((snapshot: McpSettingsSnapshot) => {
    setServers(snapshot.servers);
    setError(snapshot.error);
    setClaudeError(snapshot.claudeError);
    setLoading(false);
  }, []);

  useEffect(
    () => subscribeMcpSettings(cwd, applySnapshot),
    [cwd, applySnapshot],
  );

  const refresh = useCallback(
    async (force = true) => {
      const generation = ++refreshGeneration.current;
      const previous = getCachedMcpSettings(cwd);
      if (!force) {
        setServers(previous?.servers ?? []);
        setError(previous?.error ?? "");
        setClaudeError(previous?.claudeError ?? "");
      }
      setLoading(force || !previous);
      const snapshot = await loadMcpSettings(cwd, force);
      if (generation === refreshGeneration.current) {
        applySnapshot(getCachedMcpSettings(cwd) ?? snapshot);
      }
    },
    [cwd, applySnapshot],
  );

  useEffect(() => {
    void refresh(false);
    return () => {
      refreshGeneration.current += 1;
    };
  }, [refresh]);

  const visible = useMemo(
    () =>
      filter === "all"
        ? servers
        : servers.filter((server) => server.provider === filter),
    [filter, servers],
  );
  const filterProviders = showAllProviders
    ? PROVIDERS
    : PROVIDERS.filter((provider) =>
        servers.some((server) => server.provider === provider),
      );

  useEffect(() => {
    if (filter !== "all" && !filterProviders.includes(filter)) setFilter("all");
  }, [filter, filterProviders]);

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
          <div className="flex flex-wrap items-center gap-3">
            <h2 className="text-sm font-semibold">MCP connections</h2>
            {projectPicker}
          </div>
          <p className="mt-1 text-xs text-content/55">
            Configured servers for the selected project and your provider
            accounts.
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
            aria-label={
              showAllProviders
                ? "Show available providers"
                : "Show all providers"
            }
            aria-pressed={showAllProviders}
            title={
              showAllProviders
                ? "Showing all providers"
                : "Showing available providers"
            }
            onClick={() => setShowAllProviders(!showAllProviders)}
            className={`grid size-7 place-items-center rounded-md border border-content/10 hover:bg-content/5 ${showAllProviders ? "bg-selection text-content" : "text-content/55"}`}
          >
            <ListFilter className="size-3.5" />
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
        role="group"
        className="inline-flex max-w-full flex-wrap gap-0.5 rounded-md border border-content/10 p-0.5 text-[12px]"
        aria-label="Filter MCP servers by provider"
      >
        {(["all", ...filterProviders] as const).map((provider) => (
          <button
            key={provider}
            type="button"
            aria-pressed={filter === provider}
            onClick={() => setFilter(provider)}
            className={`inline-flex min-w-0 items-center gap-1.5 rounded-[5px] px-2.5 py-1 ${filter === provider ? "bg-selection text-content" : "text-content/50 hover:text-content"}`}
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
        <div className="overflow-hidden rounded-xl border border-content/10 bg-content/3">
          {visible.map((server) => (
            <div
              key={`${server.provider}:${server.scope}:${server.configPath}:${server.name}`}
              className="flex flex-wrap items-center gap-3 border-b border-content/5 px-4 py-3.5 last:border-b-0"
            >
              <span className="grid size-7 shrink-0 place-items-center rounded-lg bg-content/[0.05] ring-1 ring-inset ring-content/[0.06]">
                <ProviderIcon provider={server.provider} />
              </span>
              <div className="min-w-0 flex-1">
                <div className="text-[13px] font-medium text-content">
                  {server.name}
                </div>
                <div className="mt-1 text-[12px] leading-relaxed text-content/45">
                  {MCP_PROVIDER_LABELS[server.provider]} · {server.scope} ·{" "}
                  {server.transport || "MCP"} · {server.status}
                </div>
                {server.configPath ? (
                  <div
                    className="truncate text-[11px] text-content/35"
                    title={server.configPath}
                  >
                    {server.configPath}
                  </div>
                ) : null}
              </div>
              {server.provider !== "claude_desktop" &&
              server.transport &&
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

import { invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState, type FormEvent } from "react";
import { parseClaudeMcpList, type McpServer } from "../model/mcp";

type Scope = "local" | "project" | "user";

export function McpSettings({ cwd }: { cwd: string }) {
  const [servers, setServers] = useState<McpServer[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [name, setName] = useState("");
  const [config, setConfig] = useState("");
  const [scope, setScope] = useState<Scope>("local");
  const [removeScopes, setRemoveScopes] = useState<Record<string, Scope>>({});

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const output = await invoke<string>("claude_mcp_list", { cwd });
      setServers(parseClaudeMcpList(output));
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

  async function login(server: McpServer) {
    setBusy(server.name);
    setError("");
    try {
      await invoke("claude_mcp_login", { cwd, name: server.name });
      await refresh();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(null);
    }
  }

  async function remove(server: McpServer) {
    const selectedScope = removeScopes[server.name] ?? "local";
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
      <div className="flex items-center justify-between gap-3">
        <div>
          <h2 className="text-sm font-semibold">Claude Code servers</h2>
          <p className="mt-1 text-xs text-content/55">
            Connections for this project, including user and local scopes.
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
      {error ? (
        <p
          role="alert"
          className="rounded-md border border-red-500/30 bg-red-500/10 p-3 text-xs text-red-400"
        >
          {error}
        </p>
      ) : null}
      {loading ? (
        <p className="text-sm text-content/55">Checking servers…</p>
      ) : servers.length === 0 ? (
        <p className="text-sm text-content/55">No MCP servers configured.</p>
      ) : (
        <div className="divide-y divide-stroke rounded-lg border border-stroke">
          {servers.map((server) => (
            <div
              key={server.name}
              className="flex flex-wrap items-center gap-3 p-3"
            >
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium">{server.name}</div>
                <div className="text-xs text-content/55">{server.status}</div>
              </div>
              <button
                type="button"
                disabled={busy !== null}
                onClick={() => void login(server)}
                className="rounded-md border border-stroke px-2 py-1 text-xs hover:bg-content/5 disabled:opacity-50"
              >
                Sign in
              </button>
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
              <button
                type="button"
                disabled={busy !== null}
                onClick={() => void remove(server)}
                className="rounded-md border border-stroke px-2 py-1 text-xs hover:bg-content/5 disabled:opacity-50"
              >
                Remove
              </button>
            </div>
          ))}
        </div>
      )}
      <form
        onSubmit={(event) => void add(event)}
        className="space-y-3 rounded-lg border border-stroke p-4"
      >
        <div>
          <h2 className="text-sm font-semibold">Add server</h2>
          <p className="mt-1 text-xs text-content/55">
            Paste a Claude Code MCP server JSON object. HTTP servers need a type
            and URL; local servers need a command and optional args.
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
        Project servers may need approval in Claude Code before they connect.
        Sign in opens the browser for OAuth when the server supports it.
      </p>
    </div>
  );
}

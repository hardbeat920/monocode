import { useCallback, useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import { projectName } from "../../../shared/lib/paths";
import { clearInboxCache } from "../../inbox/model/githubTasks";
import {
  ASANA_CHANGE_EVENT,
  asanaConnected,
  disconnectAsana,
  linkAsanaProject,
  listAsanaProjects,
  loadAsanaProjectLinks,
  loadHiddenAsanaProjectIds,
  notifyAsanaChange,
  saveAsanaProjectLinks,
  saveAsanaToken,
  saveHiddenAsanaProjectIds,
  type AsanaProject,
  type AsanaStatus,
} from "../../inbox/model/asana";
import {
  normalizeProjectPath,
  sameProjectPath,
} from "../../projects/model/recents";

export function AsanaSettings({
  localProjects = [],
}: {
  /** MonoCode project paths an Asana project can be linked to. */
  localProjects?: readonly string[];
}) {
  const [status, setStatus] = useState<AsanaStatus | null>(null);
  const [token, setToken] = useState("");
  const [checking, setChecking] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [projects, setProjects] = useState<AsanaProject[]>([]);
  const [hiddenIds, setHiddenIds] = useState(loadHiddenAsanaProjectIds);
  const [links, setLinks] = useState(loadAsanaProjectLinks);

  const loadProjects = useCallback(async () => {
    try {
      setProjects(await listAsanaProjects());
    } catch (err) {
      setProjects([]);
      setError(String(err instanceof Error ? err.message : err));
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    void asanaConnected()
      .then(async (next) => {
        if (cancelled) return;
        setStatus(next);
        if (next.connected) await loadProjects();
      })
      .catch((err: unknown) => {
        if (!cancelled)
          setError(String(err instanceof Error ? err.message : err));
      })
      .finally(() => {
        if (!cancelled) setChecking(false);
      });
    const onChange = () => {
      setHiddenIds(loadHiddenAsanaProjectIds());
      setLinks(loadAsanaProjectLinks());
    };
    window.addEventListener(ASANA_CHANGE_EVENT, onChange);
    return () => {
      cancelled = true;
      window.removeEventListener(ASANA_CHANGE_EVENT, onChange);
    };
  }, [loadProjects]);

  const connect = async () => {
    if (busy || checking || !token.trim()) return;
    setBusy(true);
    setError(null);
    try {
      setStatus(await saveAsanaToken(token));
      setToken("");
      clearInboxCache();
      saveHiddenAsanaProjectIds([]);
      await loadProjects();
    } catch (err) {
      setError(String(err instanceof Error ? err.message : err));
    } finally {
      setBusy(false);
    }
  };

  const disconnect = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      setStatus(await disconnectAsana());
      setProjects([]);
      setToken("");
      clearInboxCache();
      notifyAsanaChange();
    } catch (err) {
      setError(String(err instanceof Error ? err.message : err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="px-4 py-3.5">
      {checking ? (
        <p className="text-[12px] text-content/45">
          Checking Asana connection…
        </p>
      ) : status?.connected ? (
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="min-w-0 text-[12px] text-content/65">
            <p className="break-all">{status.name}</p>
            <p className="break-all">{status.email}</p>
          </div>
          <SecondaryButton onClick={() => void disconnect()} disabled={busy}>
            {busy ? "Disconnecting" : "Disconnect"}
          </SecondaryButton>
        </div>
      ) : (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void connect();
          }}
          className="flex flex-col gap-3"
        >
          <p className="text-[12px] leading-relaxed text-content/45">
            Connect Asana with a personal access token from the Asana developer
            console. Disconnect deletes the saved token.
          </p>
          <label className="flex flex-col gap-1 text-[12px] text-content/65">
            Personal access token
            <input
              aria-label="Asana personal access token"
              type="password"
              value={token}
              onChange={(event) => setToken(event.target.value)}
              placeholder="Personal access token"
              disabled={busy}
              required
              autoComplete="off"
              spellCheck={false}
              className="h-8 w-full rounded-md border border-content/10 bg-transparent px-2 text-content outline-none focus:border-content/20"
            />
          </label>
          <div className="flex items-center gap-3">
            <SecondaryButton type="submit" disabled={busy || !token.trim()}>
              {busy ? "Connecting" : "Connect"}
            </SecondaryButton>
            <button
              type="button"
              onClick={() => void openUrl("https://app.asana.com/0/my-apps")}
              className="text-[12px] text-content/65 hover:text-content"
            >
              Create access token
            </button>
          </div>
        </form>
      )}
      {error ? (
        <p role="alert" className="mt-3 text-[12px] text-red-400/90">
          {error}
        </p>
      ) : null}
      {status?.connected ? (
        <div className="mt-4 flex flex-col gap-2">
          <div className="flex items-center justify-between">
            <span className="text-[13px] font-medium text-content">
              Projects
            </span>
            <SecondaryButton
              disabled={busy || checking}
              onClick={() => void loadProjects()}
            >
              Refresh projects
            </SecondaryButton>
          </div>
          <p className="text-[12px] text-content/45">
            Unchecked projects stay out of the inbox. Link a project to show
            your tasks from it when that MonoCode project is selected.
          </p>
          {projects.map((project) => (
            <div key={project.id} className="flex items-center gap-3">
              <label className="flex min-w-0 flex-1 items-center gap-2 text-[13px] text-content">
                <input
                  type="checkbox"
                  checked={!hiddenIds.includes(project.id)}
                  disabled={busy}
                  onChange={() => {
                    const next = hiddenIds.includes(project.id)
                      ? hiddenIds.filter((id) => id !== project.id)
                      : [...hiddenIds, project.id];
                    clearInboxCache();
                    saveHiddenAsanaProjectIds(next);
                  }}
                />
                <span className="truncate">
                  {project.name}{" "}
                  <span className="text-content/40">{project.key}</span>
                </span>
              </label>
              <select
                aria-label={`MonoCode project for ${project.name}`}
                value={links[project.id] ?? ""}
                disabled={busy}
                onChange={(event) => {
                  clearInboxCache();
                  saveAsanaProjectLinks(
                    linkAsanaProject(links, project.id, event.target.value),
                  );
                }}
                className="h-7 max-w-[45%] shrink-0 rounded-md border border-content/10 bg-transparent px-1.5 text-[12px] text-content outline-none focus:border-content/20"
              >
                <option value="">Not linked</option>
                {linkOptions(localProjects, links[project.id]).map((path) => (
                  <option key={path} value={path}>
                    {projectName(path)}
                  </option>
                ))}
              </select>
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}

/** Open projects, plus a linked project that is no longer in the list. */
function linkOptions(
  localProjects: readonly string[],
  linked: string | undefined,
): string[] {
  const options: string[] = [];
  for (const path of [...localProjects, ...(linked ? [linked] : [])]) {
    const normalized = normalizeProjectPath(path);
    if (!options.some((option) => sameProjectPath(option, normalized))) {
      options.push(normalized);
    }
  }
  return options;
}

import { useCallback, useEffect, useMemo, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import { PrivateEmail } from "../../../shared/ui/PrivateEmail";
import { clearInboxCache } from "../../inbox/model/githubTasks";
import {
  disconnectJira,
  JIRA_CHANGE_EVENT,
  jiraConnected,
  listJiraProjects,
  loadHiddenJiraProjectIds,
  notifyJiraChange,
  saveHiddenJiraProjectIds,
  saveJiraConfig,
  type JiraProject,
  type JiraStatus,
} from "../../inbox/model/jira";

export function JiraSettings() {
  const [status, setStatus] = useState<JiraStatus | null>(null);
  const [site, setSite] = useState("");
  const [email, setEmail] = useState("");
  const [token, setToken] = useState("");
  const [checking, setChecking] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [projects, setProjects] = useState<JiraProject[]>([]);
  const [hiddenIds, setHiddenIds] = useState(loadHiddenJiraProjectIds);
  const [projectQuery, setProjectQuery] = useState("");

  const visibleProjects = useMemo(() => {
    const query = projectQuery.trim().toLowerCase();
    if (!query) return projects;
    return projects.filter(
      (project) =>
        project.name.toLowerCase().includes(query) ||
        project.key.toLowerCase().includes(query),
    );
  }, [projects, projectQuery]);
  const selectedCount = projects.filter(
    (project) => !hiddenIds.includes(project.id),
  ).length;

  // Select / unselect all acts on the projects the search currently shows.
  const setVisibleSelected = (selected: boolean) => {
    const visibleIds = new Set(visibleProjects.map((project) => project.id));
    const kept = hiddenIds.filter((id) => !visibleIds.has(id));
    clearInboxCache();
    saveHiddenJiraProjectIds(selected ? kept : [...kept, ...visibleIds]);
  };

  const loadProjects = useCallback(async () => {
    try {
      setProjects(await listJiraProjects());
    } catch (err) {
      setProjects([]);
      setError(String(err instanceof Error ? err.message : err));
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    void jiraConnected()
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
    const onChange = () => setHiddenIds(loadHiddenJiraProjectIds());
    window.addEventListener(JIRA_CHANGE_EVENT, onChange);
    return () => {
      cancelled = true;
      window.removeEventListener(JIRA_CHANGE_EVENT, onChange);
    };
  }, [loadProjects]);

  const connect = async () => {
    if (busy || checking || !site.trim() || !token.trim()) return;
    setBusy(true);
    setError(null);
    try {
      setStatus(await saveJiraConfig({ site, email, token }));
      setToken("");
      clearInboxCache();
      saveHiddenJiraProjectIds([]);
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
      setStatus(await disconnectJira());
      setProjects([]);
      setToken("");
      clearInboxCache();
      notifyJiraChange();
    } catch (err) {
      setError(String(err instanceof Error ? err.message : err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="px-4 py-3.5">
      {checking ? (
        <p className="text-[12px] text-content/45">Checking Jira connection…</p>
      ) : status?.connected ? (
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="min-w-0 text-[12px] text-content/65">
            <p className="break-all">{status.site}</p>
            {status.email ? (
              <p className="flex min-w-0">
                <PrivateEmail key={status.email} email={status.email} />
              </p>
            ) : (
              <p>Personal access token</p>
            )}
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
            Jira Cloud: enter your Atlassian email and an API token without
            scopes. Jira Server or Data Center: leave the email empty and use a
            personal access token. Disconnect deletes the saved credentials.
          </p>
          {(
            [
              {
                label: "Jira site",
                value: site,
                set: setSite,
                type: "text",
                placeholder: "yourteam.atlassian.net or jira.company.com",
                required: true,
              },
              {
                label: "Atlassian email",
                value: email,
                set: setEmail,
                type: "email",
                placeholder: "you@example.com (Cloud only)",
                required: false,
              },
              {
                label: "Jira API token",
                value: token,
                set: setToken,
                type: "password",
                placeholder: "API token or personal access token",
                required: true,
              },
            ] as const
          ).map((field) => (
            <label
              key={field.label}
              className="flex flex-col gap-1 text-[12px] text-content/65"
            >
              {field.label}
              <input
                aria-label={field.label}
                type={field.type}
                value={field.value}
                onChange={(event) => field.set(event.target.value)}
                placeholder={field.placeholder}
                disabled={busy}
                required={field.required}
                autoComplete="off"
                spellCheck={false}
                className="h-8 w-full rounded-md border border-content/10 bg-transparent px-2 text-content outline-none focus:border-content/20"
              />
            </label>
          ))}
          <div className="flex items-center gap-3">
            <SecondaryButton
              type="submit"
              disabled={busy || !site.trim() || !token.trim()}
            >
              {busy ? "Connecting" : "Connect"}
            </SecondaryButton>
            <button
              type="button"
              onClick={() =>
                void openUrl(
                  "https://id.atlassian.com/manage-profile/security/api-tokens",
                )
              }
              className="text-[12px] text-content/65 hover:text-content"
            >
              Create API token
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
            Unchecked projects stay out of the inbox. {selectedCount} of{" "}
            {projects.length} selected.
          </p>
          {projects.length > 0 ? (
            <div className="flex flex-wrap items-center gap-2">
              <input
                type="search"
                aria-label="Search Jira projects"
                value={projectQuery}
                onChange={(event) => setProjectQuery(event.target.value)}
                placeholder="Search projects"
                autoComplete="off"
                spellCheck={false}
                className="h-8 min-w-0 flex-1 rounded-md border border-content/10 bg-transparent px-2 text-[12px] text-content outline-none focus:border-content/20"
              />
              <SecondaryButton
                disabled={busy || visibleProjects.length === 0}
                onClick={() => setVisibleSelected(true)}
              >
                Select all
              </SecondaryButton>
              <SecondaryButton
                disabled={busy || visibleProjects.length === 0}
                onClick={() => setVisibleSelected(false)}
              >
                Unselect all
              </SecondaryButton>
            </div>
          ) : null}
          {projects.length > 0 && visibleProjects.length === 0 ? (
            <p className="text-[12px] text-content/45">
              No projects match “{projectQuery.trim()}”.
            </p>
          ) : null}
          <div className="flex max-h-80 flex-col gap-2 overflow-y-auto">
            {visibleProjects.map((project) => (
              <label
                key={project.id}
                className="flex items-center gap-2 text-[13px] text-content"
              >
                <input
                  type="checkbox"
                  checked={!hiddenIds.includes(project.id)}
                  disabled={busy}
                  onChange={() => {
                    const next = hiddenIds.includes(project.id)
                      ? hiddenIds.filter((id) => id !== project.id)
                      : [...hiddenIds, project.id];
                    clearInboxCache();
                    saveHiddenJiraProjectIds(next);
                  }}
                />
                {project.name}{" "}
                <span className="text-content/40">{project.key}</span>
              </label>
            ))}
          </div>
        </div>
      ) : null}
    </div>
  );
}

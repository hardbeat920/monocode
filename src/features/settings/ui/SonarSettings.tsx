import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { SearchableSelect } from "../../../shared/ui/SearchableSelect";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import { clearInboxCache } from "../../inbox/model/githubTasks";
import {
  connectSonar,
  disconnectSonar,
  listSonarOrganizations,
  listSonarProjects,
  setSonarOrganization,
  setSonarProjects,
  sonarConnected,
  type SonarOrganization,
  type SonarProject,
  type SonarStatus,
} from "../../inbox/model/sonar";

const errorMessage = (err: unknown) =>
  String(err instanceof Error ? err.message : err);

export function SonarSettings() {
  const [status, setStatus] = useState<SonarStatus | null>(null);
  const [site, setSite] = useState("https://sonarcloud.io");
  const [token, setToken] = useState("");
  const [checking, setChecking] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    void sonarConnected()
      .then((next) => {
        if (!cancelled) setStatus(next);
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      })
      .finally(() => {
        if (!cancelled) setChecking(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);
  const connect = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const next = await connectSonar({
        site: site.trim(),
        token: token.trim(),
      });
      setStatus(next);
      setToken("");
      clearInboxCache();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };
  const disconnect = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      setStatus(await disconnectSonar());
      setToken("");
      clearInboxCache();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="px-4 py-3.5">
      {checking ? (
        <p className="text-[12px] text-content/45">
          Checking Sonar connection…
        </p>
      ) : status?.connected ? (
        <div className="flex flex-col gap-3">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="min-w-0 break-all text-[12px] text-content/65">
              {status.site}
            </p>
            <SecondaryButton disabled={busy} onClick={() => void disconnect()}>
              {busy ? "Disconnecting" : "Disconnect"}
            </SecondaryButton>
          </div>
          <SonarProjectChooser status={status} onStatus={setStatus} />
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
            Use a personal access token with Browse permission. After
            connecting, choose the projects to watch. Disconnect deletes the
            saved credentials.
          </p>
          {(
            [
              {
                label: "Sonar URL",
                value: site,
                set: setSite,
                type: "text",
                placeholder: "https://sonarcloud.io",
              },
              {
                label: "Sonar personal access token",
                value: token,
                set: setToken,
                type: "password",
                placeholder: "Personal access token",
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
                value={field.value}
                type={field.type}
                onChange={(event) => field.set(event.target.value)}
                placeholder={field.placeholder}
                required
                disabled={busy}
                autoComplete="off"
                spellCheck={false}
                className="h-8 w-full rounded-md border border-content/10 bg-transparent px-2 text-content outline-none focus:border-content/20"
              />
            </label>
          ))}
          <p className="text-[11px] text-content/45">
            For US SonarQube Cloud, use https://sonarqube.us. Self-hosted
            SonarQube URLs are also supported.
          </p>
          <div>
            <SecondaryButton
              type="submit"
              disabled={busy || !site.trim() || !token.trim()}
            >
              {busy ? "Connecting…" : "Connect Sonar"}
            </SecondaryButton>
          </div>
        </form>
      )}
      {error ? (
        <p role="alert" className="mt-3 break-words text-[12px] text-rose-400">
          {error}
        </p>
      ) : null}
    </div>
  );
}

/** Organizations exist only on SonarQube Cloud. Server lists projects directly. */
function SonarProjectChooser({
  status,
  onStatus,
}: {
  status: SonarStatus;
  onStatus: (status: SonarStatus) => void;
}) {
  const [organizations, setOrganizations] = useState<
    SonarOrganization[] | null
  >(null);
  const [projects, setProjects] = useState<SonarProject[] | null>(null);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const saving = useRef(false);
  const organization = status.organization;
  const watched = useMemo(
    () => new Set(status.projects.map((project) => project.key)),
    [status.projects],
  );
  // Only a successful empty answer means SonarQube Server. A failed read must not list all of SonarCloud.
  const scoped =
    organizations !== null && (organizations.length === 0 || !!organization);

  useEffect(() => {
    let cancelled = false;
    void listSonarOrganizations()
      .then((next) => {
        if (!cancelled) setOrganizations(next);
      })
      .catch((err) => {
        if (!cancelled) setError(errorMessage(err));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Each save replaces the stored list. It is a local file write, so a click during one is dropped rather than queued.
  const persist = useCallback(
    async (change: () => Promise<SonarStatus>) => {
      if (saving.current) return;
      saving.current = true;
      setError(null);
      try {
        onStatus(await change());
        clearInboxCache();
      } catch (err) {
        setError(errorMessage(err));
      } finally {
        saving.current = false;
      }
    },
    [onStatus],
  );

  const chooseOrganization = useCallback(
    (key: string) => void persist(() => setSonarOrganization(key)),
    [persist],
  );

  useEffect(() => {
    if (organizations?.length === 1 && !organization) {
      chooseOrganization(organizations[0]!.key);
    }
  }, [chooseOrganization, organizations, organization]);

  useEffect(() => {
    if (!scoped) return;
    let cancelled = false;
    setProjects(null);
    void listSonarProjects()
      .then((next) => {
        if (!cancelled) setProjects(next);
      })
      .catch((err) => {
        if (cancelled) return;
        setProjects([]);
        setError(errorMessage(err));
      });
    return () => {
      cancelled = true;
    };
  }, [scoped, organization]);

  // Keep watched projects the token can no longer see, so they can still be unwatched.
  const listed = useMemo(() => {
    if (!projects) return [];
    const keys = new Set(projects.map((project) => project.key));
    return [
      ...projects,
      ...status.projects.filter((project) => !keys.has(project.key)),
    ];
  }, [projects, status.projects]);

  const save = (next: Set<string>) =>
    void persist(() =>
      setSonarProjects(listed.filter((project) => next.has(project.key))),
    );

  const toggle = (key: string) => {
    const next = new Set(watched);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    save(next);
  };

  const needle = query.trim().toLocaleLowerCase();
  const visible = listed.filter(
    (project) =>
      !needle ||
      project.name.toLocaleLowerCase().includes(needle) ||
      project.key.toLocaleLowerCase().includes(needle),
  );
  // Acts on the search results, so a search can watch or unwatch a matching group.
  const visibleWatched = visible.filter((project) => watched.has(project.key));
  const allVisibleWatched =
    visible.length > 0 && visibleWatched.length === visible.length;
  const toggleVisible = () => {
    const next = new Set(watched);
    for (const project of visible) {
      if (allVisibleWatched) next.delete(project.key);
      else next.add(project.key);
    }
    save(next);
  };

  return (
    <div className="flex flex-col gap-3">
      {organizations === null ? (
        error ? null : (
          <p className="text-[12px] text-content/45">Loading organizations…</p>
        )
      ) : organizations.length > 0 ? (
        <SearchableSelect
          label="Sonar organization"
          value={organization}
          options={organizations.map((entry) => ({
            value: entry.key,
            label: entry.name,
            keywords: entry.key,
          }))}
          onChange={chooseOrganization}
          placeholder="Choose an organization…"
          searchable={organizations.length > 8}
        />
      ) : null}
      {scoped ? (
        projects === null ? (
          <p className="text-[12px] text-content/45">Loading projects…</p>
        ) : listed.length === 0 ? (
          <p className="text-[12px] text-content/45">
            No projects are visible to this token. It needs Browse permission on
            the projects to watch.
          </p>
        ) : (
          <div className="flex flex-col gap-1.5">
            <div className="flex items-center justify-between text-[12px] text-content/65">
              <label className="flex cursor-pointer items-center gap-2 px-2">
                <input
                  type="checkbox"
                  aria-label={
                    allVisibleWatched
                      ? "Unwatch all listed projects"
                      : "Watch all listed projects"
                  }
                  checked={allVisibleWatched}
                  ref={(input) => {
                    if (input)
                      input.indeterminate =
                        visibleWatched.length > 0 && !allVisibleWatched;
                  }}
                  disabled={visible.length === 0}
                  onChange={toggleVisible}
                />
                Watched projects
              </label>
              <span className="text-content/45">
                {watched.size} of {listed.length}
              </span>
            </div>
            <input
              aria-label="Search Sonar projects"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search projects"
              autoComplete="off"
              spellCheck={false}
              className="h-8 w-full rounded-md border border-content/10 bg-transparent px-2 text-[12px] text-content outline-none focus:border-content/20"
            />
            <ul className="max-h-64 overflow-y-auto rounded-md border border-content/10 py-1">
              {visible.map((project) => (
                <li key={project.key}>
                  <label
                    title={project.key}
                    className="flex cursor-pointer items-center gap-2 px-2 py-1 text-[12px] text-content/80 hover:bg-content/5"
                  >
                    <input
                      type="checkbox"
                      checked={watched.has(project.key)}
                      onChange={() => toggle(project.key)}
                    />
                    <span className="min-w-0 break-words">{project.name}</span>
                  </label>
                </li>
              ))}
              {visible.length === 0 ? (
                <li className="px-2 py-1 text-[12px] text-content/45">
                  No matching projects
                </li>
              ) : null}
            </ul>
          </div>
        )
      ) : null}
      {error ? (
        <p role="alert" className="break-words text-[12px] text-rose-400">
          {error}
        </p>
      ) : null}
    </div>
  );
}

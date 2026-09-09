import { useEffect, useMemo, useState, type ReactNode } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Copy, FolderOpen, RefreshCw, Search } from "../chrome/icons";
import { CreateSkillForm } from "../chrome/SkillPicker";
import { copyText } from "../lib/clipboard";
import { listSkills, type DiscoveredSkill } from "../lib/fs";
import {
  createBlankSkill,
  invalidateSkills,
  loadDisabledSkillPaths,
  saveDisabledSkillPaths,
  SKILLS_CHANGE_EVENT,
} from "../lib/skills";

/** Inspect and manage file skills without modifying provider-owned catalogs. */
export function SkillsPage({ cwd }: { cwd: string }): ReactNode {
  const [skills, setSkills] = useState<DiscoveredSkill[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [adding, setAdding] = useState(false);
  const [busy, setBusy] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const [disabledPaths, setDisabledPaths] = useState<string[]>(() =>
    loadDisabledSkillPaths(),
  );
  const [actionError, setActionError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setSkills(null);
    setError(null);
    listSkills(cwd)
      .then((next) => {
        if (cancelled) return;
        setSkills(next);
        setError(null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [cwd, reload]);

  useEffect(() => {
    const onChange = (): void => setDisabledPaths(loadDisabledSkillPaths());
    window.addEventListener(SKILLS_CHANGE_EVENT, onChange);
    return () => window.removeEventListener(SKILLS_CHANGE_EVENT, onChange);
  }, []);

  const needle = query.trim().toLowerCase();
  const filtered = useMemo(
    () =>
      (skills ?? []).filter(
        (skill) =>
          !needle ||
          skill.name.toLowerCase().includes(needle) ||
          skill.description.toLowerCase().includes(needle) ||
          skill.source.toLowerCase().includes(needle) ||
          skill.path.toLowerCase().includes(needle),
      ),
    [needle, skills],
  );

  const onToggle = (path: string, enabled: boolean): void => {
    const next = enabled
      ? disabledPaths.filter((item) => item !== path)
      : [...disabledPaths, path];
    try {
      saveDisabledSkillPaths(next);
      setActionError(null);
    } catch {
      setActionError("Could not save the skill preference. Try again.");
    }
  };

  const onReveal = (path: string): void => {
    setActionError(null);
    void revealItemInDir(path).catch((err: unknown) => {
      setActionError(
        `Could not open the folder: ${err instanceof Error ? err.message : String(err)}`,
      );
    });
  };

  const onCopyPath = (path: string): void => {
    setActionError(null);
    void copyText(path).catch(() => {
      setActionError("Could not copy the path to the clipboard.");
    });
  };

  const onCreate = (name: string, scope: "project" | "user"): void => {
    setBusy(true);
    setCreateError(null);
    void createBlankSkill({ cwd, name, scope })
      .then(() => {
        invalidateSkills();
        window.dispatchEvent(new Event(SKILLS_CHANGE_EVENT));
        setAdding(false);
        setReload((value) => value + 1);
      })
      .catch((err: unknown) => {
        setCreateError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => setBusy(false));
  };

  return (
    <>
      <div className="flex flex-wrap items-center justify-between gap-3 pb-3">
        <div className="flex min-w-0 flex-1 items-center gap-3">
          <span className="shrink-0 text-[12px] text-content/40 tabular-nums">
            {skills == null
              ? "…"
              : `${filtered.length} ${filtered.length === 1 ? "skill" : "skills"}`}
          </span>
          <label className="flex h-7 w-52 min-w-0 flex-1 items-center gap-2 rounded-md border border-content/10 px-2 text-content/45 focus-within:border-content/20">
            <Search className="size-3.5 shrink-0" strokeWidth={1.75} />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Filter"
              aria-label="Filter skills"
              spellCheck={false}
              autoComplete="off"
              className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/35"
            />
          </label>
          <button
            type="button"
            aria-label="Refresh skills"
            title="Rescan skill folders"
            disabled={skills === null && !error}
            onClick={() => {
              invalidateSkills();
              window.dispatchEvent(new Event(SKILLS_CHANGE_EVENT));
              setReload((value) => value + 1);
            }}
            className="grid size-6 shrink-0 place-items-center rounded-md text-content/45 hover:bg-content/10 hover:text-content"
          >
            <RefreshCw className="size-3.5" strokeWidth={1.75} />
          </button>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <button
            type="button"
            aria-label={adding ? "Close skill form" : "Add skill"}
            disabled={busy}
            className="rounded-md border border-content/10 px-2.5 py-1 text-[12px] text-content/70 hover:bg-content/10 disabled:opacity-40"
            onClick={() => {
              setAdding((value) => !value);
              setCreateError(null);
            }}
            title="Create a starter SKILL.md you can edit"
          >
            {adding ? "Close" : "Add skill"}
          </button>
        </div>
      </div>

      {adding ? (
        <div className="mb-4 overflow-hidden rounded-lg border border-content/10 bg-content/[0.03]">
          <CreateSkillForm
            key={cwd}
            query={query}
            cwd={cwd}
            error={createError}
            busy={busy}
            onCancel={() => {
              setAdding(false);
              setCreateError(null);
            }}
            onCreate={onCreate}
          />
        </div>
      ) : null}

      {actionError ? (
        <p role="alert" className="pb-3 text-[12px] text-red-400">
          {actionError}
        </p>
      ) : null}

      {error ? (
        <p role="alert" className="text-[12px] text-red-400">
          {error}
        </p>
      ) : skills == null ? (
        <p className="text-[12px] text-content/45">Loading skills…</p>
      ) : (
        <div className="overflow-hidden rounded-lg border border-content/10">
          {filtered.length === 0 ? (
            <p className="px-3 py-3 text-[12px] text-content/45">
              {skills.length === 0
                ? "No skills yet. Add skill creates a starter SKILL.md."
                : "No matching skills"}
            </p>
          ) : (
            filtered.map((skill) => {
              const disabled = disabledPaths.includes(skill.path);
              return (
                <div
                  key={skill.path}
                  className={`border-b border-content/5 px-3 py-2 last:border-b-0 ${
                    disabled ? "opacity-50" : ""
                  }`}
                >
                  <div className="flex items-center gap-2">
                    <span
                      className="min-w-0 flex-1 truncate font-mono text-[12px] text-content"
                      title={skill.name}
                    >
                      {skill.name}
                    </span>
                    <span className="shrink-0 rounded-full bg-content/10 px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide text-content/60">
                      {skill.scope === "user"
                        ? "Personal"
                        : skill.scope === "builtin"
                          ? "MonoCode"
                          : "Project"}
                    </span>
                    <span className="w-20 shrink-0 truncate text-right font-mono text-[11px] text-content/40">
                      {skill.source}
                    </span>
                    <button
                      type="button"
                      role="switch"
                      aria-label={`Include ${skill.name} in MonoCode catalog`}
                      aria-checked={!disabled}
                      onClick={() => onToggle(skill.path, disabled)}
                      className={`relative h-5 w-9 shrink-0 rounded-full transition-colors ${disabled ? "bg-content/20" : "bg-accent"}`}
                    >
                      <span
                        className={`absolute top-0.5 size-4 rounded-full bg-white transition-[left] ${disabled ? "left-0.5" : "left-4.5"}`}
                      />
                    </button>
                  </div>
                  {skill.description ? (
                    <p
                      className="mt-0.5 truncate text-[12px] text-content/55"
                      title={skill.description}
                    >
                      {skill.description}
                    </p>
                  ) : null}
                  <div className="mt-0.5 flex items-center gap-1">
                    <p
                      className="min-w-0 flex-1 truncate font-mono text-[11px] text-content/35"
                      title={skill.path}
                    >
                      {skill.path}
                    </p>
                    <button
                      type="button"
                      aria-label={`Copy path of ${skill.name}`}
                      title="Copy path"
                      onClick={() => onCopyPath(skill.path)}
                      className="grid size-5 shrink-0 place-items-center rounded text-content/40 hover:bg-content/10 hover:text-content"
                    >
                      <Copy className="size-3" strokeWidth={1.75} />
                    </button>
                    <button
                      type="button"
                      aria-label={`Reveal ${skill.name} in file explorer`}
                      title="Reveal in file manager"
                      onClick={() => onReveal(skill.path)}
                      className="grid size-5 shrink-0 place-items-center rounded text-content/40 hover:bg-content/10 hover:text-content"
                    >
                      <FolderOpen className="size-3" strokeWidth={1.75} />
                    </button>
                  </div>
                </div>
              );
            })
          )}
        </div>
      )}

      <p className="pt-3 text-[12px] text-content/40">
        Hidden skills stay on disk and are excluded from MonoCode's file-skill
        catalog. Provider-managed skills and native commands are unaffected.
        Skills live in <span className="font-mono">.agents/skills</span> for
        this project and <span className="font-mono">~/.agents/skills</span> for
        you personally; harness folders are also picked up.
      </p>
    </>
  );
}

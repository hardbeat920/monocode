import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Copy, Eye, FolderOpen, RefreshCw, Search, X } from "../chrome/icons";
import { CreateSkillForm } from "../chrome/SkillPicker";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import {
  MarkdownModeToggle,
  useMarkdownMode,
} from "../chrome/MarkdownModeToggle";
import { MarkdownSource } from "./AgentMarkdown";
import { SkillDocumentPreview } from "./SkillDocumentPreview";
import { copyText } from "../lib/clipboard";
import { listSkills, readTextFile, type DiscoveredSkill } from "../lib/fs";
import {
  createBlankSkill,
  invalidateSkills,
  loadDisabledSkillPaths,
  saveDisabledSkillPaths,
  SKILLS_CHANGE_EVENT,
} from "../lib/skills";

/** Inspect and manage file skills without modifying provider-owned catalogs. */
export function SkillsPage({
  cwd,
  header,
}: {
  cwd: string;
  header?: ReactNode;
}): ReactNode {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const previewId = useId();
  const previewOpener = useRef<string | null>(null);
  const previewButtons = useRef(new Map<string, HTMLButtonElement>());
  const filterInput = useRef<HTMLInputElement>(null);
  const closePreview = useRef<HTMLButtonElement>(null);
  const addSkillButton = useRef<HTMLButtonElement>(null);
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
  const [previewSkill, setPreviewSkill] = useState<DiscoveredSkill | null>(
    null,
  );
  const [previewText, setPreviewText] = useState<string | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [previewMode, setPreviewMode] = useMarkdownMode(
    `skill:${previewSkill?.path ?? ""}`,
  );
  const previewOpen = previewSkill !== null;

  const onPreview = (
    skill: DiscoveredSkill,
    trigger: "name" | "icon",
  ): void => {
    previewOpener.current = `${trigger}:${skill.path}`;
    if (previewSkill?.path !== skill.path) {
      setPreviewText(null);
      setPreviewError(null);
    }
    setPreviewSkill(skill);
  };

  const registerPreviewButton =
    (key: string) =>
    (button: HTMLButtonElement | null): void => {
      if (button) previewButtons.current.set(key, button);
      else previewButtons.current.delete(key);
    };

  useEffect(() => {
    if (!previewOpen) return;
    closePreview.current?.focus();
    return () => {
      // A rescan replaces row elements, so restore by skill and trigger identity.
      const opener = previewButtons.current.get(previewOpener.current ?? "");
      const target = opener ?? filterInput.current;
      if (target?.isConnected) target.focus();
    };
  }, [previewOpen]);

  useEffect(() => {
    let cancelled = false;
    setPreviewText(null);
    setPreviewError(null);
    if (!previewSkill) return;
    void readTextFile(previewSkill.path)
      .then((text) => {
        if (!cancelled) setPreviewText(text);
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setPreviewError(
            `Could not read SKILL.md. ${err instanceof Error ? err.message : String(err)}`,
          );
        }
      });
    return () => {
      cancelled = true;
    };
  }, [previewSkill]);

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
    <div
      className="@container/skills flex min-h-0 min-w-0 flex-1"
      onKeyDown={(event) => {
        if (!previewOpen || event.key !== "Escape" || event.defaultPrevented)
          return;
        event.preventDefault();
        event.stopPropagation();
        setPreviewSkill(null);
      }}
    >
      <div className="flex min-h-0 min-w-0 flex-1 flex-col @3xl/skills:flex-row">
        <div
          ref={lockOverscroll}
          className="min-h-0 min-w-0 flex-1 overflow-y-auto overscroll-none"
        >
          <div
            className={`mx-auto w-full max-w-5xl py-8 ${previewOpen ? "px-4" : "px-8"}`}
          >
            {header}
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
                    ref={filterInput}
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
                  ref={addSkillButton}
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
                  monospace={false}
                  error={createError}
                  busy={busy}
                  onCancel={() => {
                    setAdding(false);
                    setCreateError(null);
                    addSkillButton.current?.focus();
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
                        className={`border-b border-content/5 px-3 py-2 last:border-b-0 ${previewSkill?.path === skill.path ? "bg-content/5" : ""} ${
                          disabled ? "opacity-50" : ""
                        }`}
                      >
                        <div className="flex items-center gap-2">
                          <button
                            type="button"
                            className="mr-auto min-w-0 truncate rounded text-left font-sans text-[12px] text-content hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                            title={`Preview ${skill.name}`}
                            ref={registerPreviewButton(`name:${skill.path}`)}
                            aria-controls={previewOpen ? previewId : undefined}
                            aria-expanded={previewSkill?.path === skill.path}
                            onClick={() => onPreview(skill, "name")}
                          >
                            {skill.name}
                          </button>
                          <span className="shrink-0 rounded-full bg-content/10 px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide text-content/60">
                            {skill.scope === "user"
                              ? "Personal"
                              : skill.scope === "builtin"
                                ? "MonoCode"
                                : "Project"}
                          </span>
                          <span className="w-20 shrink-0 truncate text-right font-sans text-[11px] text-content/40">
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
                            className="min-w-0 flex-1 truncate font-sans text-[11px] text-content/35"
                            title={skill.path}
                          >
                            {skill.path}
                          </p>
                          <button
                            type="button"
                            aria-label={`Preview skill ${skill.name}`}
                            title="Preview skill"
                            ref={registerPreviewButton(`icon:${skill.path}`)}
                            aria-controls={previewOpen ? previewId : undefined}
                            aria-expanded={previewSkill?.path === skill.path}
                            onClick={() => onPreview(skill, "icon")}
                            className="grid size-5 shrink-0 place-items-center rounded text-content/40 hover:bg-content/10 hover:text-content focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                          >
                            <Eye
                              className="size-3"
                              strokeWidth={1.75}
                              aria-hidden="true"
                            />
                          </button>
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
              Hidden skills stay on disk and are excluded from MonoCode's
              file-skill catalog. Provider-managed skills and native commands
              are unaffected. Skills live in{" "}
              <span className="font-sans">.agents/skills</span> for this project
              and <span className="font-sans">~/.agents/skills</span> for you
              personally; harness folders are also picked up.
            </p>
          </div>
        </div>
        {previewSkill ? (
          <aside
            id={previewId}
            aria-label="Skill preview"
            className="flex min-h-0 min-w-0 flex-1 flex-col border-t border-content/10 @3xl/skills:max-w-[720px] @3xl/skills:border-t-0 @3xl/skills:border-l"
          >
            <header className="flex shrink-0 items-start gap-2 px-4 pt-4 pb-2">
              <h2 className="min-w-0 flex-1 break-words text-[16px] font-semibold text-content">
                {previewSkill.name}
              </h2>
              <button
                ref={closePreview}
                type="button"
                aria-label="Close skill preview"
                title="Close preview (Escape)"
                onClick={() => setPreviewSkill(null)}
                className="grid size-6 shrink-0 place-items-center rounded-md text-content/45 hover:bg-content/10 hover:text-content focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              >
                <X className="size-3.5" strokeWidth={1.75} />
              </button>
            </header>
            <div className="shrink-0 space-y-3 border-b border-content/10 px-4 pt-1 pb-3">
              <p className="select-text break-all text-[11px] text-content/50">
                {previewSkill.path}
              </p>
              <div className="flex justify-end">
                <MarkdownModeToggle
                  mode={previewMode}
                  onChange={setPreviewMode}
                />
              </div>
            </div>
            <div
              key={previewSkill.path}
              className="min-h-0 min-w-0 flex-1 select-text"
            >
              {previewError ? (
                <p
                  role="alert"
                  className="break-words px-4 py-5 text-[12px] text-red-400"
                >
                  {previewError}
                </p>
              ) : previewText === null ? (
                <p
                  role="status"
                  className="px-4 py-5 text-[12px] text-content/50"
                >
                  Loading skill…
                </p>
              ) : previewMode === "preview" ? (
                <SkillDocumentPreview text={previewText} />
              ) : (
                <MarkdownSource text={previewText} />
              )}
            </div>
          </aside>
        ) : null}
      </div>
    </div>
  );
}

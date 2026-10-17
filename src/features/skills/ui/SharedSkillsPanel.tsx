import { useEffect, useId, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { FolderOpen, RefreshCw } from "../../../shared/ui/icons";
import { readTextFile } from "../../../platform/tauri/fs";
import {
  applySharedSkill,
  importSharedSkill,
  repairSharedSkills,
  setSharedSkillSharing,
  sharedSkillsSnapshot,
  type SharedSkillEntry,
  type SharedSkillsSnapshot,
} from "../../../platform/tauri/sharedSkills";
import { invalidateSkills, SKILLS_CHANGE_EVENT } from "../model/skills";
import { SkillDocumentPreview } from "./SkillDocumentPreview";

const buttonClass =
  "inline-flex h-7 items-center justify-center gap-1.5 rounded-md border border-content/10 px-2.5 text-[12px] text-content/70 hover:bg-content/5 hover:text-content disabled:cursor-default disabled:opacity-40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent";

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function exportSummary(entry: SharedSkillEntry): string {
  if (!entry.shared) return "Sharing off";
  const conflicts = entry.statuses.filter(
    (status) => status.state === "conflict",
  ).length;
  if (conflicts)
    return `${conflicts} ${conflicts === 1 ? "conflict" : "conflicts"}`;
  const exported = entry.statuses.filter(
    (status) => status.state === "exported",
  ).length;
  return exported ? `Shared with ${exported} folders` : "Pending export";
}

function AppliedSkillPreview({ entry }: { entry: SharedSkillEntry }) {
  const [document, setDocument] = useState<{
    key: string;
    text?: string;
    error?: string;
  } | null>(null);
  const key = `${entry.id}:${entry.revision}:${entry.previewPath}`;
  useEffect(() => {
    let cancelled = false;
    void readTextFile(entry.previewPath)
      .then((text) => {
        if (!cancelled) setDocument({ key, text });
      })
      .catch((error: unknown) => {
        if (!cancelled) setDocument({ key, error: errorMessage(error) });
      });
    return () => {
      cancelled = true;
    };
  }, [key, entry.previewPath]);

  // Key the result as well as the request so an old document never appears
  // under a new skill before React runs the effect.
  const current = document?.key === key ? document : null;
  return (
    <details className="mt-3 rounded-lg border border-content/10">
      <summary className="cursor-pointer px-3 py-2 text-[12px] text-content/65 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent">
        Applied instructions
      </summary>
      <div
        aria-label={`Applied instructions for ${entry.name}`}
        className="h-72 min-w-0 select-text border-t border-content/10"
      >
        {current?.error ? (
          <p
            role="alert"
            className="break-words px-3 py-3 text-[12px] text-red-400"
          >
            Could not read the applied skill. {current.error}
          </p>
        ) : current?.text !== undefined ? (
          <SkillDocumentPreview text={current.text} />
        ) : (
          <p role="status" className="px-3 py-3 text-[12px] text-content/45">
            Loading applied instructions...
          </p>
        )}
      </div>
    </details>
  );
}

/** Manage owned skill copies without replacing files owned by providers. */
export function SharedSkillsPanel() {
  const headingId = useId();
  const [snapshot, setSnapshot] = useState<SharedSkillsSnapshot | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [importPath, setImportPath] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<string | null>(
    "Loading shared library...",
  );
  const mounted = useRef(false);
  const busy = useRef(false);
  const request = useRef(0);
  const acceptedGeneration = useRef(-1);

  const acceptSnapshot = (next: SharedSkillsSnapshot): void => {
    if (next.generation < acceptedGeneration.current) return;
    acceptedGeneration.current = next.generation;
    setSnapshot(next);
    setSelectedId((previous) =>
      next.entries.some((entry) => entry.id === previous)
        ? previous
        : (next.entries[0]?.id ?? null),
    );
  };

  useEffect(() => {
    mounted.current = true;
    const current = ++request.current;
    busy.current = true;
    void sharedSkillsSnapshot()
      .then((next) => {
        if (mounted.current && current === request.current)
          acceptSnapshot(next);
      })
      .catch((failure: unknown) => {
        if (mounted.current && current === request.current)
          setError(errorMessage(failure));
      })
      .finally(() => {
        if (mounted.current && current === request.current) {
          busy.current = false;
          setPending(null);
        }
      });
    return () => {
      mounted.current = false;
      request.current += 1;
    };
  }, []);

  const run = async (
    label: string,
    action: () => Promise<SharedSkillsSnapshot | null>,
    changed = true,
  ): Promise<void> => {
    if (busy.current) return;
    busy.current = true;
    const current = ++request.current;
    setPending(label);
    setError(null);
    try {
      const next = await action();
      if (next && changed) {
        // A completed mutation must invalidate catalogs even if Settings closes.
        invalidateSkills();
        window.dispatchEvent(new Event(SKILLS_CHANGE_EVENT));
      }
      if (next && mounted.current && current === request.current)
        acceptSnapshot(next);
    } catch (failure) {
      if (mounted.current && current === request.current)
        setError(errorMessage(failure));
    } finally {
      if (mounted.current && current === request.current) {
        busy.current = false;
        setPending(null);
      }
    }
  };

  const importFolder = (): void => {
    void run("Importing skill...", async () => {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Import skill folder",
      });
      if (typeof selected !== "string" || !selected) return null;
      if (mounted.current) setImportPath(selected);
      return importSharedSkill(selected);
    });
  };
  const importFromPath = (): void => {
    const path = importPath.trim();
    if (path) void run("Importing skill...", () => importSharedSkill(path));
  };
  const selected = snapshot?.entries.find((entry) => entry.id === selectedId);
  const disabled = pending !== null;

  return (
    <section
      aria-labelledby={headingId}
      className="mb-7 rounded-lg border border-content/10 bg-content/[0.02]"
    >
      <div className="border-b border-content/10 px-3 py-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h2 id={headingId} className="text-[13px] font-semibold text-content">
            Shared library
          </h2>
          <div className="flex items-center gap-2">
            <button
              type="button"
              className={buttonClass}
              disabled={disabled}
              onClick={() =>
                void run(
                  "Refreshing shared library...",
                  sharedSkillsSnapshot,
                  false,
                )
              }
            >
              <RefreshCw
                className="size-3"
                strokeWidth={1.75}
                aria-hidden="true"
              />
              Refresh library
            </button>
            <button
              type="button"
              className={buttonClass}
              disabled={disabled}
              onClick={() =>
                void run("Repairing sharing...", repairSharedSkills)
              }
            >
              Repair sharing
            </button>
          </div>
        </div>
        <p className="mt-1 text-[12px] leading-5 text-content/50">
          Import once and share complete skill folders across providers on this
          machine. Existing provider files stay protected. Running sessions may
          need a restart to load changes.
        </p>
        <div className="mt-3 flex flex-wrap gap-2">
          <input
            aria-label="Skill folder path"
            placeholder="Folder containing SKILL.md"
            value={importPath}
            disabled={disabled}
            spellCheck={false}
            autoComplete="off"
            onChange={(event) => setImportPath(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                importFromPath();
              }
            }}
            className="h-7 min-w-0 flex-1 rounded-md border border-content/10 bg-transparent px-2 text-[12px] text-content outline-none placeholder:text-content/35 focus:border-content/20 disabled:opacity-40"
          />
          <button
            type="button"
            className={buttonClass}
            disabled={disabled || !importPath.trim()}
            onClick={importFromPath}
          >
            Import path
          </button>
          <button
            type="button"
            className={buttonClass}
            disabled={disabled}
            onClick={importFolder}
          >
            <FolderOpen
              className="size-3"
              strokeWidth={1.75}
              aria-hidden="true"
            />
            Choose folder
          </button>
        </div>
        {pending ? (
          <p role="status" className="mt-2 text-[12px] text-content/50">
            {pending}
          </p>
        ) : null}
        {error ? (
          <p role="alert" className="mt-2 break-words text-[12px] text-red-400">
            {error}
          </p>
        ) : null}
      </div>
      {snapshot ? (
        <div className="flex min-w-0 flex-col @2xl/skills:flex-row">
          <div className="max-h-72 min-w-0 overflow-y-auto @2xl/skills:w-56 @2xl/skills:shrink-0">
            {snapshot.entries.length ? (
              snapshot.entries.map((entry) => (
                <button
                  key={entry.id}
                  type="button"
                  aria-label={`Select shared skill ${entry.name}`}
                  aria-pressed={entry.id === selectedId}
                  onClick={() => setSelectedId(entry.id)}
                  className={`block w-full border-b border-content/5 px-3 py-2 text-left last:border-b-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent ${entry.id === selectedId ? "bg-content/5" : "hover:bg-content/[0.03]"}`}
                >
                  <span className="block truncate text-[12px] text-content">
                    {entry.name}
                  </span>
                  <span
                    className={`mt-0.5 block text-[11px] ${entry.statuses.some((status) => status.state === "conflict") ? "text-amber-500" : "text-content/45"}`}
                  >
                    {exportSummary(entry)}
                  </span>
                </button>
              ))
            ) : (
              <p className="px-3 py-3 text-[12px] text-content/45">
                No shared skills yet. Import a folder to start.
              </p>
            )}
          </div>
          {selected ? (
            <div className="min-w-0 flex-1 border-t border-content/10 px-3 py-3 @2xl/skills:border-t-0 @2xl/skills:border-l">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <h3 className="min-w-0 break-words text-[13px] font-medium text-content">
                  {selected.name}
                </h3>
                <button
                  type="button"
                  role="switch"
                  aria-label={`Share ${selected.name} across providers`}
                  aria-checked={selected.shared}
                  className={buttonClass}
                  disabled={disabled}
                  onClick={() =>
                    void run(
                      selected.shared
                        ? "Stopping sharing..."
                        : "Starting sharing...",
                      () =>
                        setSharedSkillSharing(selected.id, !selected.shared),
                    )
                  }
                >
                  {selected.shared ? "Stop sharing" : "Start sharing"}
                </button>
              </div>
              <p className="mt-1 break-words text-[12px] text-content/55">
                {selected.description}
              </p>
              <div className="mt-3 flex flex-wrap items-center gap-2">
                <button
                  type="button"
                  className={buttonClass}
                  disabled={disabled}
                  onClick={() =>
                    void run("Applying edits...", () =>
                      applySharedSkill(selected.id),
                    )
                  }
                >
                  Apply edits
                </button>
                <button
                  type="button"
                  className={buttonClass}
                  disabled={disabled}
                  onClick={() => {
                    setError(null);
                    void revealItemInDir(selected.sourcePath).catch(
                      (failure: unknown) => {
                        if (mounted.current)
                          setError(
                            `Could not open the source folder. ${errorMessage(failure)}`,
                          );
                      },
                    );
                  }}
                >
                  <FolderOpen
                    className="size-3"
                    strokeWidth={1.75}
                    aria-hidden="true"
                  />
                  Open source folder
                </button>
                <span className="text-[11px] text-content/40">
                  Revision {selected.revision}
                </span>
              </div>
              <p className="mt-2 break-words text-[11px] text-content/40">
                Edit files in the source folder, then apply edits to update
                shared copies.
              </p>
              <p className="mt-1 select-text break-all font-sans text-[11px] text-content/35">
                {selected.sourcePath}
              </p>
              {selected.warnings.map((warning) => (
                <p
                  key={warning}
                  className="mt-2 break-words text-[12px] text-amber-500"
                >
                  {warning}
                </p>
              ))}
              <ul
                aria-label={`Sharing status for ${selected.name}`}
                className="mt-3 space-y-2"
              >
                {selected.statuses.map((status) => (
                  <li
                    key={status.targetKey}
                    className="rounded-md border border-content/10 px-2.5 py-2"
                  >
                    <div className="flex flex-wrap items-baseline justify-between gap-1 text-[11px]">
                      <span className="text-content/70">
                        {status.providers.join(", ")}
                      </span>
                      <span
                        className={
                          status.state === "conflict"
                            ? "text-amber-500"
                            : "text-content/45"
                        }
                      >
                        {status.state === "exported"
                          ? "Exported"
                          : status.state === "disabled"
                            ? "Sharing off"
                            : status.state === "unsupported"
                              ? "Unsupported"
                              : status.state === "conflict"
                                ? "Conflict"
                                : "Pending"}
                      </span>
                    </div>
                    <p className="mt-1 break-words text-[11px] text-content/45">
                      {status.detail}
                    </p>
                    {status.path ? (
                      <p className="mt-1 select-text break-all font-sans text-[10px] text-content/35">
                        {status.path}
                      </p>
                    ) : null}
                  </li>
                ))}
              </ul>
              <AppliedSkillPreview entry={selected} />
            </div>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}

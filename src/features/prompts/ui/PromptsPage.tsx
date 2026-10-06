import { useEffect, useMemo, useState, type ReactNode } from "react";
import { Pencil, Pin, Search, Trash2, X } from "../../../shared/ui/icons";
import { useLockOverscroll } from "../../../shared/hooks/useLockOverscroll";
import {
  deleteSavedPrompt,
  rankSavedPrompts,
  savedPromptLabel,
  saveSavedPrompt,
  type SavedPrompt,
} from "../model/savedPrompts";
import { SavedPromptForm, type SavedPromptDraft } from "./SavedPromptForm";
import { useSavedPrompts } from "./useSavedPrompts";

/** Editing an existing prompt, or `"new"` for the Add prompt form. */
type Editing = SavedPrompt | "new" | null;

const EMPTY: SavedPromptDraft = { title: "", body: "", pinned: false };

/**
 * Settings → Prompts: the saved-prompt library. A list with a filter on the
 * left; the editor opens beside it (below it when narrow), like Skills.
 */
export function PromptsPage({ header }: { header?: ReactNode }): ReactNode {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const { prompts, error, reload } = useSavedPrompts();
  const [query, setQuery] = useState("");
  const [editing, setEditing] = useState<Editing>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  const filtered = useMemo(
    () =>
      prompts ? rankSavedPrompts(prompts, query, Number.POSITIVE_INFINITY) : [],
    [prompts, query],
  );

  const run = async (action: () => Promise<unknown>): Promise<void> => {
    setActionError(null);
    try {
      await action();
      reload();
    } catch (reason) {
      setActionError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const save = async (draft: SavedPromptDraft): Promise<void> => {
    const id = editing && editing !== "new" ? editing.id : crypto.randomUUID();
    await saveSavedPrompt({ id, ...draft });
    reload();
    setEditing(null);
  };

  const editingId = editing && editing !== "new" ? editing.id : null;

  // Escape closes the editor first, before Settings' own Escape handler.
  const editorOpen = editing !== null;
  useEffect(() => {
    if (!editorOpen) return;
    const onKey = (event: KeyboardEvent): void => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      event.stopPropagation();
      setEditing(null);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [editorOpen]);

  return (
    <div className="@container/prompts flex min-h-0 min-w-0 flex-1">
      <div className="flex min-h-0 min-w-0 flex-1 flex-col @3xl/prompts:flex-row">
        <div
          ref={lockOverscroll}
          className="min-h-0 min-w-0 flex-1 overflow-y-auto overscroll-none"
        >
          <div
            className={`mx-auto w-full max-w-5xl py-8 ${editing ? "px-4" : "px-8"}`}
          >
            {header}
            <div className="flex flex-wrap items-center justify-between gap-3 pb-3">
              <div className="flex min-w-0 flex-1 items-center gap-3">
                <span className="shrink-0 text-[12px] text-content/40 tabular-nums">
                  {prompts == null
                    ? "…"
                    : `${filtered.length} ${filtered.length === 1 ? "prompt" : "prompts"}`}
                </span>
                <label className="flex h-7 w-52 min-w-0 flex-1 items-center gap-2 rounded-md border border-content/10 px-2 text-content/45 focus-within:border-content/20">
                  <Search className="size-3.5 shrink-0" strokeWidth={1.75} />
                  <input
                    value={query}
                    onChange={(event) => setQuery(event.target.value)}
                    placeholder="Filter"
                    aria-label="Filter prompts"
                    spellCheck={false}
                    autoComplete="off"
                    className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/35"
                  />
                </label>
              </div>
              <button
                type="button"
                onClick={() => setEditing("new")}
                className="rounded-md border border-content/10 px-2.5 py-1 text-[12px] text-content/70 hover:bg-content/10"
              >
                Add prompt
              </button>
            </div>

            {actionError ? (
              <p role="alert" className="pb-3 text-[12px] text-red-400">
                {actionError}
              </p>
            ) : null}

            {error && prompts == null ? (
              <p role="alert" className="text-[12px] text-red-400">
                Could not load prompts. {error}
              </p>
            ) : prompts == null ? (
              <p className="text-[12px] text-content/45">Loading prompts…</p>
            ) : (
              <ul
                aria-label="Saved prompts"
                className="overflow-hidden rounded-lg border border-content/10"
              >
                {filtered.length === 0 ? (
                  <li className="px-3 py-3 text-[12px] text-content/45">
                    {prompts.length === 0
                      ? "No prompts yet. Add prompt saves a keyword, phrase or prompt to reuse."
                      : "No matching prompts"}
                  </li>
                ) : (
                  filtered.map((prompt) => {
                    const label = savedPromptLabel(prompt);
                    const confirming = confirmDelete === prompt.id;
                    return (
                      <li
                        key={prompt.id}
                        className={`border-b border-content/5 px-3 py-2 last:border-b-0 ${
                          editingId === prompt.id ? "bg-content/5" : ""
                        }`}
                      >
                        <div className="flex items-center gap-1">
                          <button
                            type="button"
                            onClick={() => setEditing(prompt)}
                            title={`Edit ${label}`}
                            className="mr-auto min-w-0 truncate rounded text-left text-[12px] text-content hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                          >
                            {label}
                          </button>
                          {prompt.useCount > 0 ? (
                            <span className="shrink-0 pr-1 text-[11px] text-content/35 tabular-nums">
                              used {prompt.useCount}×
                            </span>
                          ) : null}
                          <button
                            type="button"
                            aria-label={
                              prompt.pinned ? `Unpin ${label}` : `Pin ${label}`
                            }
                            aria-pressed={prompt.pinned}
                            title={prompt.pinned ? "Unpin" : "Pin to the top"}
                            onClick={() =>
                              void run(() =>
                                saveSavedPrompt({
                                  id: prompt.id,
                                  title: prompt.title,
                                  body: prompt.body,
                                  pinned: !prompt.pinned,
                                }),
                              )
                            }
                            className={`grid size-6 shrink-0 place-items-center rounded hover:bg-content/10 ${
                              prompt.pinned
                                ? "text-content"
                                : "text-content/35 hover:text-content"
                            }`}
                          >
                            <Pin className="size-3.5" />
                          </button>
                          <button
                            type="button"
                            aria-label={`Edit ${label}`}
                            title="Edit"
                            onClick={() => setEditing(prompt)}
                            className="grid size-6 shrink-0 place-items-center rounded text-content/35 hover:bg-content/10 hover:text-content"
                          >
                            <Pencil className="size-3.5" />
                          </button>
                          {confirming ? (
                            <button
                              type="button"
                              autoFocus
                              onBlur={() => setConfirmDelete(null)}
                              onClick={() => {
                                setConfirmDelete(null);
                                if (editingId === prompt.id) setEditing(null);
                                void run(() => deleteSavedPrompt(prompt.id));
                              }}
                              className="h-6 shrink-0 rounded px-2 text-[11px] font-medium text-red-400 hover:bg-red-400/10"
                            >
                              Delete?
                            </button>
                          ) : (
                            <button
                              type="button"
                              aria-label={`Delete ${label}`}
                              title="Delete"
                              onClick={() => setConfirmDelete(prompt.id)}
                              className="grid size-6 shrink-0 place-items-center rounded text-content/35 hover:bg-content/10 hover:text-red-400"
                            >
                              <Trash2 className="size-3.5" />
                            </button>
                          )}
                        </div>
                        <p className="mt-0.5 line-clamp-2 whitespace-pre-line text-[12px] text-content/55">
                          {prompt.body}
                        </p>
                      </li>
                    );
                  })
                )}
              </ul>
            )}

            <p className="pt-3 text-[12px] text-content/40">
              Type <kbd className="font-sans text-content/60">!</kbd> in any
              composer to insert a prompt; keep typing to filter. You can also
              select text in a reply and choose Add to Prompts. Pinned prompts
              stay on top, then the ones you use most.
            </p>
          </div>
        </div>
        {editing ? (
          <aside
            aria-label={editing === "new" ? "New prompt" : "Edit prompt"}
            className="flex min-h-0 min-w-0 flex-1 flex-col border-t border-stroke @3xl/prompts:max-w-[560px] @3xl/prompts:border-t-0 @3xl/prompts:border-l"
          >
            <header className="flex shrink-0 items-center gap-2 px-4 pt-4 pb-3">
              <h2 className="min-w-0 flex-1 truncate text-[16px] font-semibold text-content">
                {editing === "new" ? "New prompt" : "Edit prompt"}
              </h2>
              <button
                type="button"
                aria-label="Close editor"
                title="Close"
                onClick={() => setEditing(null)}
                className="grid size-6 shrink-0 place-items-center rounded-md text-content/45 hover:bg-content/10 hover:text-content"
              >
                <X className="size-3.5" strokeWidth={1.75} />
              </button>
            </header>
            <div className="min-h-0 flex-1 overflow-y-auto px-4 pb-4">
              <SavedPromptForm
                key={editing === "new" ? "new" : editing.id}
                initial={
                  editing === "new"
                    ? EMPTY
                    : {
                        title: editing.title,
                        body: editing.body,
                        pinned: editing.pinned,
                      }
                }
                onCancel={() => setEditing(null)}
                onSave={save}
              />
            </div>
          </aside>
        ) : null}
      </div>
    </div>
  );
}

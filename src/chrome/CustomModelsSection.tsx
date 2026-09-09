import {
  useEffect,
  useId,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import {
  MAX_CUSTOM_MODEL_COUNT,
  normalizeCustomModelSlug,
  type CustomModelHarness,
} from "../lib/customModels";
import {
  addCustomModel,
  getModelSnapshot,
  loadCustomModels,
  providerModelsFor,
  removeCustomModel,
  subscribeModels,
  updateCustomModel,
} from "../lib/models";
import { HARNESS_TITLE } from "../lib/session";
import { CustomModelEditor } from "./CustomModelEditor";
import { ChevronDown, Pencil, Plus, X } from "./icons";

const buttonClass =
  "flex items-center gap-1.5 rounded-md px-2 py-1 text-[12px] text-content/50 hover:bg-content/10 hover:text-content focus-visible:outline focus-visible:outline-accent";

export function CustomModelsSection({
  harness,
}: {
  harness: CustomModelHarness;
}) {
  const version = useSyncExternalStore(
    subscribeModels,
    getModelSnapshot,
    getModelSnapshot,
  );
  const id = useId();
  const entries = loadCustomModels(harness);
  const [expanded, setExpanded] = useState(false);
  const [adding, setAdding] = useState(false);
  const [input, setInput] = useState("");
  const [filter, setFilter] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [editingSlug, setEditingSlug] = useState<string | null>(null);
  const addButton = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const scrollToSlug = useRef<string | null>(null);
  const showFilter = entries.length > 8;
  const needle = showFilter ? filter.trim().toLowerCase() : "";
  const visible = entries.filter((entry) =>
    `${entry.name} ${entry.slug}`.toLowerCase().includes(needle),
  );

  useEffect(() => {
    if (!scrollToSlug.current) return;
    const row = list.current?.querySelector<HTMLElement>(
      `[data-custom-model-slug="${CSS.escape(scrollToSlug.current)}"]`,
    );
    if (!row) return;
    row.scrollIntoView({ block: "nearest" });
    scrollToSlug.current = null;
  }, [version]);

  const cancelAdd = () => {
    setAdding(false);
    setInput("");
    setError(null);
    addButton.current?.focus();
  };

  return (
    <section
      aria-label={`${HARNESS_TITLE[harness]} custom models`}
      className="pb-3"
    >
      <div className="flex items-center justify-between gap-2 pt-2">
        <button
          type="button"
          className={buttonClass}
          aria-expanded={expanded}
          aria-controls={`${id}-models`}
          onClick={() => setExpanded((current) => !current)}
        >
          <ChevronDown className={`size-3.5 ${expanded ? "" : "-rotate-90"}`} />
          Custom models
          {entries.length ? (
            <span className="rounded bg-content/10 px-1.5 text-[10px] tabular-nums">
              {entries.length}
            </span>
          ) : null}
        </button>
        <button
          ref={addButton}
          type="button"
          className={`${buttonClass} disabled:opacity-40`}
          disabled={entries.length >= MAX_CUSTOM_MODEL_COUNT}
          title={
            entries.length >= MAX_CUSTOM_MODEL_COUNT
              ? `Up to ${MAX_CUSTOM_MODEL_COUNT} custom models per provider`
              : undefined
          }
          onClick={() => {
            setExpanded(true);
            setAdding(true);
            setEditingSlug(null);
            setError(null);
          }}
        >
          <Plus className="size-3.5" />
          Add model
        </button>
      </div>

      {expanded ? (
        <div id={`${id}-models`} className="space-y-2 px-2 pt-2">
          <p className="text-[12px] leading-relaxed text-content/40">
            Add a model ID supported by your {HARNESS_TITLE[harness]} CLI
            configuration. Models appear in the picker and can be used by
            default.
          </p>
          {adding ? (
            <form
              data-custom-model-editor
              className="flex flex-wrap items-center gap-2 py-1"
              onKeyDown={(event) => {
                if (event.key !== "Escape" || event.nativeEvent.isComposing)
                  return;
                event.preventDefault();
                event.stopPropagation();
                cancelAdd();
              }}
              onSubmit={(event) => {
                event.preventDefault();
                const problem = addCustomModel(harness, input);
                setError(problem);
                if (problem) return;
                scrollToSlug.current = normalizeCustomModelSlug(input);
                setFilter("");
                cancelAdd();
              }}
            >
              <input
                autoFocus
                aria-label={`${HARNESS_TITLE[harness]} custom model ID`}
                aria-invalid={!!error}
                aria-describedby={error ? `${id}-error` : undefined}
                value={input}
                spellCheck={false}
                autoComplete="off"
                placeholder={
                  harness === "codex" ? "my-codex-model" : "my-claude-model"
                }
                className="min-w-0 flex-1 rounded-md border border-content/10 bg-content/5 px-2.5 py-1.5 font-mono text-[12px] text-content outline-none placeholder:text-content/30 focus:border-content/30"
                onChange={(event) => {
                  setInput(event.target.value);
                  setError(null);
                }}
              />
              <button
                type="submit"
                className={`${buttonClass} border border-content/10 bg-content/5`}
              >
                Add
              </button>
              <button
                type="button"
                aria-label="Cancel adding model"
                className={buttonClass}
                onClick={cancelAdd}
              >
                <X className="size-3.5" />
              </button>
            </form>
          ) : null}
          {error ? (
            <p
              id={`${id}-error`}
              role="alert"
              className="text-[12px] text-red-400"
            >
              {error}
            </p>
          ) : null}
          {showFilter ? (
            <input
              aria-label={`Filter ${HARNESS_TITLE[harness]} custom models`}
              value={filter}
              placeholder="Filter custom models…"
              className="w-full rounded-md border border-content/10 bg-content/5 px-2.5 py-1.5 text-[12px] text-content outline-none placeholder:text-content/30 focus:border-content/30"
              onChange={(event) => setFilter(event.target.value)}
            />
          ) : null}
          <div ref={list} className="divide-y divide-content/5">
            {visible.map((entry) => (
              <div
                key={entry.slug}
                data-custom-model-slug={entry.slug}
                className="py-2"
              >
                <div className="flex min-w-0 items-center gap-2">
                  <div className="min-w-0 flex-1">
                    <div
                      className="truncate text-[12px] text-content/80"
                      title={entry.name}
                    >
                      {entry.name}
                    </div>
                    {entry.name !== entry.slug ? (
                      <div
                        className="mt-0.5 truncate font-mono text-[11px] text-content/40"
                        title={entry.slug}
                      >
                        {entry.slug}
                      </div>
                    ) : null}
                  </div>
                  <button
                    type="button"
                    aria-label={`Edit ${entry.name}`}
                    className={buttonClass}
                    onClick={() => {
                      setEditingSlug(entry.slug);
                      setAdding(false);
                      setError(null);
                    }}
                  >
                    <Pencil className="size-3.5" />
                  </button>
                  <button
                    type="button"
                    aria-label={`Remove ${entry.name}`}
                    className={`${buttonClass} hover:text-red-400`}
                    onClick={() => {
                      const problem = removeCustomModel(harness, entry.slug);
                      setError(problem);
                      if (!problem && editingSlug === entry.slug)
                        setEditingSlug(null);
                    }}
                  >
                    <X className="size-3.5" />
                  </button>
                </div>
                {editingSlug === entry.slug ? (
                  <CustomModelEditor
                    harness={harness}
                    entry={entry}
                    builtInModels={providerModelsFor(harness)}
                    onCancel={() => setEditingSlug(null)}
                    onSave={(next) => {
                      const problem = updateCustomModel(harness, next);
                      if (!problem) setEditingSlug(null);
                      return problem;
                    }}
                  />
                ) : null}
              </div>
            ))}
            {needle && visible.length === 0 ? (
              <p className="py-2 text-[12px] text-content/40">
                No matching models.
              </p>
            ) : null}
          </div>
        </div>
      ) : null}
    </section>
  );
}

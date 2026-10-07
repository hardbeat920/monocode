import { useRef, useState } from "react";
import { isImeComposition } from "../../../shared/lib/keyboard";
import { IconButton } from "../../../app/shell/TitleBar";
import { Pencil, Plus, StickyNote, Trash2 } from "../../../shared/ui/icons";
import type { MonoLook } from "../model/mono";
import {
  MonoFileConflict,
  editMonoFile,
  type MonoFiles,
} from "../model/monoFiles";
import {
  addMemoryEntry,
  memoryDate,
  memoryEntry,
  memoryLines,
  withLineEdited,
  withoutLine,
  type MemoryLine,
} from "../model/monoMemory";
import { HabitButton } from "./MonoHabits";
import {
  AutoTextarea,
  Empty,
  FileField,
  MemoryGauge,
  PageHeader,
} from "./monoPanelParts";

const dateFormat = new Intl.DateTimeFormat(undefined, {
  month: "short",
  day: "numeric",
});

function shortDate(date: string): string {
  const [year, month, day] = date.split("-").map(Number);
  return dateFormat.format(new Date(year, month - 1, day));
}

type MemoryFact = MemoryLine & { line: string };

/** Locate the original fact after other writers have inserted or moved rows. */
function factIndex(memory: string, original: string): number {
  const indices = memory
    .split("\n")
    .flatMap((line, index) => (line === original ? [index] : []));
  if (indices.length !== 1)
    throw new Error(
      "This memory changed or is ambiguous. Check it before trying again.",
    );
  return indices[0];
}

/** The soul as its markdown source, editable in place; saved when you leave it. */
export function SoulPage({
  monoId,
  agent,
  files,
  onBack,
}: {
  monoId: string;
  agent: MonoLook;
  files: MonoFiles | undefined;
  onBack: () => void;
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-mono-soul>
      <PageHeader title="Soul" onBack={onBack} />
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-none">
        {files ? (
          <FileField
            key={`${monoId}:soul`}
            monoId={monoId}
            file="soul"
            value={files.soul}
            hash={files.soulHash}
            label={`${agent.name} soul`}
            author={agent.name}
            placeholder="Who it is, and what it should always keep in mind"
          />
        ) : (
          <Empty>Loading…</Empty>
        )}
      </div>
    </div>
  );
}

/** What the Mono remembers, one fact per row; click a fact to reword it. */
export function MemoryPage({
  monoId,
  files,
  onBack,
}: {
  monoId: string;
  files: MonoFiles | undefined;
  onBack: () => void;
}) {
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<MemoryFact>();
  const [saveError, setSaveError] = useState<string>();
  const lines = files?.memory.split("\n") ?? [];
  const facts: MemoryFact[] = files
    ? memoryLines(files.memory).map((fact) => ({
        ...fact,
        line: lines[fact.index],
      }))
    : [];
  // Keep the editor and its draft even if a reload removes the original row.
  if (editing && !facts.some((fact) => fact.line === editing.line))
    facts.splice(Math.min(editing.index, facts.length), 0, editing);
  const editingIndex = editing
    ? facts.findIndex((fact) => fact.line === editing.line)
    : -1;
  const save = async (edit: (memory: string) => string): Promise<boolean> => {
    try {
      await editMonoFile(monoId, "memory", edit);
      setSaveError(undefined);
      return true;
    } catch (error) {
      setSaveError(
        error instanceof MonoFileConflict
          ? "Memory kept changing while saving. Your edit was not saved; try again."
          : error instanceof Error
            ? error.message
            : String(error),
      );
      return false;
    }
  };
  const forget = (line: string) =>
    save((memory) => withoutLine(memory, factIndex(memory, line)));
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-mono-memory>
      <PageHeader title="Memory" onBack={onBack}>
        {files ? (
          <IconButton
            label="Add memory"
            disabled={adding}
            onClick={() => {
              setEditing(undefined);
              setAdding(true);
            }}
          >
            <Plus className="size-3.5" strokeWidth={1.75} />
          </IconButton>
        ) : null}
      </PageHeader>
      {saveError ? (
        <p role="alert" className="px-4 py-2 text-[12px] text-red-400">
          {saveError}
        </p>
      ) : null}
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-none px-2 py-2">
        {!files ? (
          <Empty>Loading…</Empty>
        ) : facts.length === 0 && !adding ? (
          <Empty>Nothing remembered yet</Empty>
        ) : (
          <ul className="flex flex-col gap-px">
            {adding ? (
              <li>
                <FactEditor
                  label="New memory"
                  initial=""
                  placeholder="Something it should remember"
                  // Enter saves and leaves the field open for the next one.
                  keepOpen
                  onSave={(fact) =>
                    save(
                      (memory) =>
                        addMemoryEntry(memory, memoryEntry(fact, memoryDate()))
                          .text,
                    )
                  }
                  onClose={() => setAdding(false)}
                />
              </li>
            ) : null}
            {facts.map((fact, index) => {
              const edit = editingIndex === index ? editing : undefined;
              return edit ? (
                <li key={`editing:${edit.line}`} data-memory-line={fact.index}>
                  <FactEditor
                    label="Edit memory"
                    initial={edit.text}
                    onSave={(text) =>
                      text === edit.text
                        ? Promise.resolve(true)
                        : save((memory) =>
                            withLineEdited(
                              memory,
                              factIndex(memory, edit.line),
                              text,
                              memoryDate(),
                            ),
                          )
                    }
                    onClose={() =>
                      setEditing((current) =>
                        current === edit ? undefined : current,
                      )
                    }
                  />
                </li>
              ) : (
                <FactRow
                  key={fact.index}
                  index={fact.index}
                  text={fact.text}
                  date={fact.date}
                  struck={fact.struck}
                  onEdit={() => {
                    setAdding(false);
                    setEditing(fact);
                  }}
                  onForget={() => {
                    void forget(fact.line);
                  }}
                />
              );
            })}
          </ul>
        )}
      </div>
      {files ? (
        <div className="shrink-0 border-t border-stroke px-2 pb-3">
          <MemoryGauge memory={files.memory} />
        </div>
      ) : null}
    </div>
  );
}

function FactIcon({ dim = false }: { dim?: boolean }) {
  return (
    <span
      aria-hidden
      className={`grid size-7 shrink-0 place-items-center rounded-md bg-content/6 ${
        dim ? "text-content/30" : "text-content/60"
      }`}
    >
      <StickyNote className="size-3.5" strokeWidth={1.75} />
    </span>
  );
}

/** One fact, laid out like a habit: its text, when it was noted, and actions on hover. */
function FactRow({
  index,
  text,
  date,
  struck,
  onEdit,
  onForget,
}: {
  index: number;
  text: string;
  date?: string;
  struck?: boolean;
  onEdit: () => void;
  onForget: () => void;
}) {
  const body = (
    <>
      <FactIcon dim={struck} />
      <span className="flex min-w-0 flex-1 flex-col">
        <span
          className={`truncate text-[12px] leading-5 ${
            struck ? "text-content/35 line-through" : "text-content/85"
          }`}
        >
          {text}
        </span>
        {date ? (
          <span className="text-[11px] leading-4 text-content/40">
            {shortDate(date)}
          </span>
        ) : null}
      </span>
    </>
  );
  return (
    <li
      data-memory-line={index}
      className="group/fact flex items-center rounded-md hover:bg-content/5"
    >
      {struck ? (
        <div className="flex min-w-0 flex-1 items-center gap-2.5 px-2 py-2">
          {body}
        </div>
      ) : (
        <button
          type="button"
          onClick={onEdit}
          className="flex min-w-0 flex-1 items-center gap-2.5 rounded-md px-2 py-2 text-left"
        >
          {body}
        </button>
      )}
      <span className="flex shrink-0 items-center pr-2 opacity-0 transition-opacity group-hover/fact:opacity-100 focus-within:opacity-100">
        {struck ? null : (
          <HabitButton label="Edit" onClick={onEdit}>
            <Pencil className="size-3.5" strokeWidth={1.75} />
          </HabitButton>
        )}
        <HabitButton label={`Forget: ${text}`} onClick={onForget}>
          <Trash2 className="size-3.5" strokeWidth={1.75} />
        </HabitButton>
      </span>
    </li>
  );
}

/**
 * A fact typed in place, in a field that grows with it. Enter or leaving the
 * field saves it; Escape closes it as it was.
 */
function FactEditor({
  label,
  initial,
  placeholder,
  keepOpen = false,
  onSave,
  onClose,
}: {
  label: string;
  initial: string;
  placeholder?: string;
  /** Clear the field after a save instead of closing it, for adding several. */
  keepOpen?: boolean;
  onSave: (fact: string) => Promise<boolean>;
  onClose: () => void;
}) {
  const [draft, setDraft] = useState(initial);
  const [saving, setSaving] = useState(false);
  const closed = useRef(false);
  const pending = useRef(false);
  const closeAfterSave = useRef(false);
  const commit = async (close: boolean) => {
    if (closed.current) return;
    if (pending.current) {
      if (close) closeAfterSave.current = true;
      return;
    }
    closeAfterSave.current = close;
    const fact = draft.replace(/\s+/g, " ").trim();
    if (fact) {
      pending.current = true;
      setSaving(true);
      const saved = await onSave(fact);
      pending.current = false;
      setSaving(false);
      if (!saved || closed.current) return;
    }
    if (keepOpen && fact && !closeAfterSave.current) {
      setDraft("");
      return;
    }
    closed.current = true;
    onClose();
  };
  return (
    <div className="flex items-start gap-2.5 rounded-md bg-content/5 px-2 py-2">
      <FactIcon />
      <AutoTextarea
        autoFocus
        aria-label={label}
        value={draft}
        readOnly={saving}
        aria-busy={saving}
        rows={1}
        maxLength={1000}
        placeholder={placeholder}
        onFocus={(event) => event.currentTarget.select()}
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && !isImeComposition(event.nativeEvent)) {
            event.preventDefault();
            void commit(false);
          }
          if (event.key === "Escape") {
            closed.current = true;
            onClose();
          }
        }}
        onBlur={() => {
          void commit(true);
        }}
        className="block min-h-7 min-w-0 flex-1 resize-none bg-transparent py-[5px] text-[12px] leading-[18px] text-content/90 outline-none placeholder:text-content/35"
      />
    </div>
  );
}

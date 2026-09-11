import { LoaderCircle, Plus, Search, File, Trash2, X } from "../chrome/icons";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  Fragment,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent as ReactDragEvent,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { useMarkdownMode } from "../chrome/MarkdownModeToggle";
import { ProjectLogoIcon } from "../chrome/ProjectLogoIcon";
import { ProjectMascot } from "../chrome/ProjectMascot";
import { OverlayNav } from "../chrome/TitleBar";
import { WindowControls } from "../chrome/WindowControls";
import { useDragResize } from "../hooks/useDragResize";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import { useTabGroupLogos } from "../hooks/useTabGroupLogos";
import { formatRelativeTime } from "../lib/githubTasks";
import {
  createNote,
  deleteNote,
  loadNotes,
  MAX_NOTE_TAGS,
  normalizeNoteTags,
  notePreview,
  noteSourceProject,
  noteTitle,
  upsertNote,
  requestAddNoteToChat,
  type Note,
} from "../lib/notes";
import {
  insertNoteImagesMarkdown,
  saveNoteImagesFromFiles,
  saveNoteImagesFromPaths,
  type NoteImageAsset,
} from "../lib/noteImages";
import { projectKey, projectName } from "../lib/paths";
import { IS_MAC } from "../lib/platform";
import { looksLikeProject } from "../lib/recents";
import {
  loadTabGroupColors,
  loadTabGroupCustomColors,
  loadTabGroupMascots,
  resolveTabGroupColor,
  resolveTabGroupLogo,
  resolveTabGroupMascot,
} from "../lib/tabGroups";
import { AgentMarkdown, MarkdownSourceHighlight } from "./AgentMarkdown";

const MIN_WIDTH = 240;
const MAX_WIDTH = 420;
const DEFAULT_WIDTH = 280;

let rememberedWidth = DEFAULT_WIDTH;
let rememberedNoteId: string | null = null;

type Props = {
  besideRail?: boolean;
  cwd?: string;
  onClose: () => void;
  onToggleSidebar?: () => void;
};

export function NotesView({
  besideRail = false,
  cwd,
  onClose,
  onToggleSidebar,
}: Props) {
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const listLock = useLockOverscroll<HTMLDivElement>();
  const resize = useDragResize({
    min: MIN_WIDTH,
    max: () => Math.min(MAX_WIDTH, Math.round(window.innerWidth * 0.5)),
    defaultWidth: DEFAULT_WIDTH,
    initial: rememberedWidth,
    onCommit: (width) => {
      rememberedWidth = width;
    },
  });
  const [notes, setNotes] = useState<Note[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(rememberedNoteId);
  const [creating, setCreating] = useState(false);
  const logos = useTabGroupLogos();
  const [groupMascots] = useState(loadTabGroupMascots);
  const [groupColors] = useState(loadTabGroupColors);
  const [groupCustomColors] = useState(loadTabGroupCustomColors);

  const refresh = useCallback(async () => {
    try {
      const next = await loadNotes(true);
      setNotes(next);
      setError(null);
      setSelectedId((current) => {
        const preferred = current ?? rememberedNoteId;
        if (preferred && next.some((note) => note.id === preferred)) {
          return preferred;
        }
        return next[0]?.id ?? null;
      });
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    rememberedNoteId = selectedId;
  }, [selectedId]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      onCloseRef.current();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return notes;
    return notes.filter((note) => {
      const project = noteSourceProject(note.sourceCwd)?.toLowerCase() ?? "";
      return (
        note.title.toLowerCase().includes(needle) ||
        note.body.toLowerCase().includes(needle) ||
        note.slug.toLowerCase().includes(needle) ||
        note.tags.some((tag) => tag.includes(needle.replace(/^#/, ""))) ||
        project.includes(needle)
      );
    });
  }, [notes, query]);

  const selected =
    visible.find((note) => note.id === selectedId) ??
    notes.find((note) => note.id === selectedId) ??
    null;

  const onCreate = async () => {
    if (creating) return;
    setCreating(true);
    try {
      const note = await createNote({
        title: "Untitled",
        body: "",
        ...(cwd && looksLikeProject(cwd) ? { sourceCwd: cwd } : {}),
      });
      setNotes(await loadNotes(true));
      setSelectedId(note.id);
      setQuery("");
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setCreating(false);
    }
  };

  const onSaved = (note: Note) => {
    setNotes((current) => {
      const next = current.map((item) => (item.id === note.id ? note : item));
      next.sort(
        (a, b) => b.updatedAt - a.updatedAt || a.id.localeCompare(b.id),
      );
      return next;
    });
  };

  const onDelete = async (id: string) => {
    try {
      await deleteNote(id);
      const next = await loadNotes(true);
      setNotes(next);
      setSelectedId((current) => {
        if (current !== id) return current;
        return next[0]?.id ?? null;
      });
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const onAddToChat = (note: Note) => {
    requestAddNoteToChat(note);
    onClose();
  };

  const list = (
    <div
      ref={resize.setPaneRef}
      className="relative flex h-full min-h-0 shrink-0 flex-col border-r border-content/10"
    >
      <div className="flex h-9 shrink-0 items-center gap-1 border-b border-content/10 px-2">
        <div className="relative flex h-7 min-w-0 flex-1 items-center">
          <Search className="pointer-events-none absolute left-2 size-3 shrink-0 opacity-50" />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Filter notes"
            aria-label="Filter notes"
            spellCheck={false}
            autoComplete="off"
            className="h-7 w-full rounded-md bg-transparent pl-7 pr-2 text-[12px] text-content outline-none placeholder:text-content/40"
          />
        </div>
        <button
          type="button"
          title="New note"
          aria-label="New note"
          disabled={creating}
          onClick={() => void onCreate()}
          className="grid size-6 shrink-0 place-items-center rounded-md text-content/45 hover:bg-content/10 hover:text-content disabled:opacity-40"
        >
          {creating ? (
            <LoaderCircle
              className="size-3.5 animate-spin"
              strokeWidth={1.75}
            />
          ) : (
            <Plus className="size-3.5" strokeWidth={1.75} />
          )}
        </button>
      </div>
      <div
        ref={listLock}
        className="min-h-0 flex-1 overflow-y-auto overscroll-none"
      >
        {error && notes.length === 0 ? (
          <p className="px-3 py-2 text-[12px] text-content/50">{error}</p>
        ) : loading && notes.length === 0 ? (
          <div className="flex justify-center py-10 text-content/40">
            <LoaderCircle className="size-4 animate-spin" strokeWidth={1.75} />
          </div>
        ) : visible.length === 0 ? (
          <p className="px-3 py-2 text-[12px] text-content/50">
            {query.trim()
              ? "No matching notes"
              : "No notes yet. Save a turn from the transcript, or create one here."}
          </p>
        ) : (
          <ul className="flex flex-col gap-0.5 p-1.5">
            {visible.map((note) => (
              <li key={note.id}>
                <NoteCard
                  note={note}
                  active={selected?.id === note.id}
                  logos={logos}
                  mascots={groupMascots}
                  colors={groupColors}
                  customColors={groupCustomColors}
                  onSelect={() => setSelectedId(note.id)}
                />
              </li>
            ))}
          </ul>
        )}
      </div>
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize notes list"
        className={`absolute inset-y-0 -right-px z-10 w-1.5 cursor-col-resize touch-none ${
          resize.dragging ? "bg-content/15" : "hover:bg-content/10"
        }`}
        onPointerDown={resize.onPointerDown}
        onDoubleClick={resize.onDoubleClick}
      />
    </div>
  );

  return (
    <div
      role="region"
      aria-label="Notes"
      data-app-notes
      className="flex min-h-0 min-w-0 flex-1 flex-col text-content"
    >
      <div
        className="flex h-10 shrink-0 select-none items-center border-b border-content/10"
        data-tauri-drag-region="deep"
      >
        {IS_MAC && !besideRail ? <div className="w-[78px] shrink-0" /> : null}
        {besideRail ? null : (
          <OverlayNav onBack={onClose} onToggleSidebar={onToggleSidebar} />
        )}
        <div className="flex min-w-0 flex-1 items-center gap-2 px-3 text-[13px]">
          <File
            className="size-3.5 shrink-0 text-content/45"
            strokeWidth={1.75}
          />
          <span className="min-w-0 truncate text-content">Notes</span>
        </div>
        {IS_MAC ? null : <WindowControls />}
      </div>
      <div className="flex min-h-0 min-w-0 flex-1">
        {list}
        <NoteDetail
          note={selected}
          logos={logos}
          mascots={groupMascots}
          colors={groupColors}
          customColors={groupCustomColors}
          onSaved={onSaved}
          onDelete={onDelete}
          onAddToChat={onAddToChat}
        />
      </div>
    </div>
  );
}

type ProjectMarks = {
  logos: Record<string, string>;
  mascots: Record<string, string>;
  colors: Record<string, number>;
  customColors: Record<string, string>;
};

function NoteProjectMark({
  cwd,
  logos,
  mascots,
  colors,
  customColors,
}: { cwd: string } & ProjectMarks) {
  const project = projectName(cwd);
  const key = projectKey(cwd);
  const logoPath = resolveTabGroupLogo(key, logos);
  const mascotName = resolveTabGroupMascot(key, mascots);
  const mascotColor = resolveTabGroupColor(key, colors, customColors, project);
  return (
    <span className="flex min-w-0 items-center gap-1.5">
      {logoPath ? (
        <ProjectLogoIcon
          path={logoPath}
          className="size-3.5 shrink-0 rounded-sm"
          imageClassName="size-3.5"
        />
      ) : (
        <ProjectMascot
          project={project}
          color={mascotColor}
          name={mascotName}
          className="size-3 shrink-0"
        />
      )}
      <span className="min-w-0 truncate">{project}</span>
    </span>
  );
}

function NoteDetailTab({
  label,
  selected,
  onSelect,
}: {
  label: string;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={selected}
      onClick={onSelect}
      className={`relative flex h-9 items-center text-[12px] leading-none ${
        selected ? "text-content" : "text-content/50 hover:text-content"
      }`}
    >
      {label}
      {selected ? (
        <span className="absolute inset-x-0 bottom-0 h-0.5 bg-content" />
      ) : null}
    </button>
  );
}

function NoteCard({
  note,
  active,
  logos,
  mascots,
  colors,
  customColors,
  onSelect,
}: {
  note: Note;
  active: boolean;
  onSelect: () => void;
} & ProjectMarks) {
  const preview = notePreview(note.body, note.title);
  const project = noteSourceProject(note.sourceCwd);
  const time = formatRelativeTime(new Date(note.updatedAt).toISOString());
  const hint = [note.title, project].filter(Boolean).join(" · ");
  return (
    <button
      type="button"
      title={hint}
      aria-current={active ? "true" : undefined}
      onClick={onSelect}
      className={`flex w-full flex-col rounded-md border px-2.5 py-2 text-left ${
        active
          ? "border-transparent bg-content/10 text-content"
          : "border-transparent text-content/80 hover:bg-content/5 hover:text-content"
      }`}
    >
      <span className="flex items-center gap-2">
        {project && note.sourceCwd ? (
          <span className="min-w-0 flex-1 text-[11px] text-content/50">
            <NoteProjectMark
              cwd={note.sourceCwd}
              logos={logos}
              mascots={mascots}
              colors={colors}
              customColors={customColors}
            />
          </span>
        ) : (
          <span className="min-w-0 flex-1" />
        )}
        {time ? (
          <span className="shrink-0 text-[11px] tabular-nums text-content/45">
            {time}
          </span>
        ) : null}
      </span>
      <span className="mt-1 line-clamp-1 text-[13px] font-semibold leading-snug text-content">
        {note.title}
      </span>
      {preview ? (
        <span className="mt-1 line-clamp-1 text-[12px] leading-snug text-content/45">
          {preview}
        </span>
      ) : null}
      {note.tags.length > 0 ? (
        <span className="mt-1.5 flex min-w-0 items-center gap-1 overflow-hidden">
          {note.tags.slice(0, 3).map((tag) => (
            <span
              key={tag}
              className="max-w-24 truncate rounded bg-content/8 px-1.5 py-0.5 text-[10px] leading-none text-content/55"
            >
              #{tag}
            </span>
          ))}
          {note.tags.length > 3 ? (
            <span className="shrink-0 text-[10px] text-content/40">
              +{note.tags.length - 3}
            </span>
          ) : null}
        </span>
      ) : null}
    </button>
  );
}

function NoteDetail({
  note,
  logos,
  mascots,
  colors,
  customColors,
  onSaved,
  onDelete,
  onAddToChat,
}: {
  note: Note | null;
  onSaved: (note: Note) => void;
  onDelete: (id: string) => void | Promise<void>;
  onAddToChat: (note: Note) => void;
} & ProjectMarks) {
  if (!note) {
    return (
      <div className="flex h-full min-w-0 flex-1 flex-col items-center justify-center px-6 text-center">
        <File className="mb-3 size-6 text-content/30" strokeWidth={1.75} />
        <p className="text-[13px] text-content/45">Select a note</p>
      </div>
    );
  }
  return (
    <NoteEditor
      key={note.id}
      note={note}
      logos={logos}
      mascots={mascots}
      colors={colors}
      customColors={customColors}
      onSaved={onSaved}
      onDelete={onDelete}
      onAddToChat={onAddToChat}
    />
  );
}

function NoteEditor({
  note,
  logos,
  mascots,
  colors,
  customColors,
  onSaved,
  onDelete,
  onAddToChat,
}: {
  note: Note;
  onSaved: (note: Note) => void;
  onDelete: (id: string) => void | Promise<void>;
  onAddToChat: (note: Note) => void;
} & ProjectMarks) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const blank = !note.body.trim() && note.title === "Untitled";
  const [mode, setMode] = useMarkdownMode(note.id);
  const [title, setTitle] = useState(note.title);
  const [body, setBody] = useState(note.body);
  const [tags, setTags] = useState(note.tags);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [imageDrag, setImageDrag] = useState(false);
  const [imageBusy, setImageBusy] = useState(false);
  const titleRef = useRef(title);
  const bodyRef = useRef(body);
  const tagsRef = useRef(tags);
  const noteRef = useRef(note);
  const dropZoneRef = useRef<HTMLDivElement>(null);
  const sourceFieldRef = useRef<HTMLTextAreaElement>(null);
  const lastDropAt = useRef(0);
  const skipSave = useRef(false);
  const saveTimer = useRef<number | null>(null);
  const saveQueue = useRef(Promise.resolve());
  const onSavedRef = useRef(onSaved);
  titleRef.current = title;
  bodyRef.current = body;
  tagsRef.current = tags;
  noteRef.current = note;
  onSavedRef.current = onSaved;
  const project = noteSourceProject(note.sourceCwd);
  const time = formatRelativeTime(new Date(note.updatedAt).toISOString());

  useEffect(() => {
    if (blank) setMode("source");
    // New untitled notes open in source so typing isn't behind the preview.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const persist = useCallback(async () => {
    if (skipSave.current) return;
    const current = noteRef.current;
    const nextTitle = titleRef.current.trim() || noteTitle(bodyRef.current);
    const nextBody = bodyRef.current;
    const nextTags = tagsRef.current;
    if (
      nextTitle === current.title &&
      nextBody === current.body &&
      sameTags(nextTags, current.tags)
    )
      return;
    try {
      const saved = await upsertNote({
        id: current.id,
        title: nextTitle,
        body: nextBody,
        tags: nextTags,
      });
      setSaveError(null);
      if (
        titleRef.current.trim() === "" ||
        titleRef.current === current.title
      ) {
        setTitle(saved.title);
      }
      onSavedRef.current(saved);
    } catch (err: unknown) {
      setSaveError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  const scheduleSave = useCallback(() => {
    if (saveTimer.current != null) window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      saveTimer.current = null;
      saveQueue.current = saveQueue.current.then(persist, persist);
    }, 400);
  }, [persist]);

  const insertionRange = useCallback(() => {
    const field = sourceFieldRef.current;
    if (!field) {
      const end = bodyRef.current.length;
      return { start: end, end };
    }
    return {
      start: field.selectionStart,
      end: field.selectionEnd,
    };
  }, []);

  const addDroppedImages = useCallback(
    async (
      load: () => Promise<NoteImageAsset[]>,
      range: { start: number; end: number },
    ) => {
      setImageBusy(true);
      setImageDrag(false);
      try {
        const images = await load();
        const inserted = insertNoteImagesMarkdown(
          bodyRef.current,
          range.start,
          range.end,
          images,
        );
        bodyRef.current = inserted.value;
        setBody(inserted.value);
        setSaveError(null);
        scheduleSave();
        window.requestAnimationFrame(() => {
          const field = sourceFieldRef.current;
          if (!field) return;
          field.focus();
          field.setSelectionRange(inserted.cursor, inserted.cursor);
        });
      } catch (err: unknown) {
        setSaveError(err instanceof Error ? err.message : String(err));
      } finally {
        setImageBusy(false);
      }
    },
    [scheduleSave],
  );

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    const toClientPoint = (x: number, y: number) => {
      const scale = window.devicePixelRatio || 1;
      if (scale !== 1 && (x > window.innerWidth || y > window.innerHeight)) {
        return { x: x / scale, y: y / scale };
      }
      return { x, y };
    };
    const overDropZone = (x: number, y: number) => {
      const zone = dropZoneRef.current;
      if (!zone) return false;
      const point = toClientPoint(x, y);
      const rect = zone.getBoundingClientRect();
      return (
        point.x >= rect.left &&
        point.x <= rect.right &&
        point.y >= rect.top &&
        point.y <= rect.bottom
      );
    };

    void getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "leave") {
          setImageDrag(false);
          return;
        }
        const { x, y } = event.payload.position;
        const over = overDropZone(x, y);
        if (event.payload.type === "enter" || event.payload.type === "over") {
          setImageDrag(over);
          return;
        }
        if (event.payload.type !== "drop") return;
        setImageDrag(false);
        if (!over || Date.now() - lastDropAt.current < 250) return;
        lastDropAt.current = Date.now();
        const range = insertionRange();
        const paths = event.payload.paths;
        void addDroppedImages(
          () => saveNoteImagesFromPaths(note.id, paths),
          range,
        );
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => undefined);

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [addDroppedImages, insertionRange, note.id]);

  useEffect(() => {
    return () => {
      if (saveTimer.current != null) window.clearTimeout(saveTimer.current);
      void persist();
    };
  }, [persist]);

  const onTitleKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    event.currentTarget.blur();
  };

  const canAddToChat = Boolean(body.trim());
  const draft: Note = {
    ...note,
    title: title.trim() || noteTitle(body),
    body,
    tags,
  };

  return (
    <div
      ref={lockOverscroll}
      className="min-h-0 min-w-0 flex-1 overflow-y-auto overscroll-none"
    >
      <div className="mx-auto flex w-full max-w-5xl flex-col gap-5 px-8 py-8">
        <header className="flex flex-col gap-3">
          <div className="flex min-w-0 items-center gap-2 text-[12px] text-content/50">
            <File className="size-3.5 shrink-0" strokeWidth={1.75} />
            <span>Note</span>
            {note.slug ? (
              <span className="min-w-0 truncate">{note.slug}</span>
            ) : null}
            {project && note.sourceCwd ? (
              <NoteProjectMark
                cwd={note.sourceCwd}
                logos={logos}
                mascots={mascots}
                colors={colors}
                customColors={customColors}
              />
            ) : null}
          </div>
          <input
            value={title}
            onChange={(event) => {
              setTitle(event.target.value);
              scheduleSave();
            }}
            onBlur={() => {
              const next = title.trim() || noteTitle(body);
              if (next !== title) setTitle(next);
              void persist();
            }}
            onKeyDown={onTitleKeyDown}
            aria-label="Note title"
            className="w-full border-0 bg-transparent p-0 text-[20px] font-semibold leading-tight text-content outline-none placeholder:text-content/35"
            placeholder="Untitled"
          />
          {time ? (
            <div className="text-[12px] text-content/50">Updated {time}</div>
          ) : null}
          <NoteTagsEditor
            tags={tags}
            onChange={(next) => {
              tagsRef.current = next;
              setTags(next);
              scheduleSave();
            }}
          />
          <div className="flex flex-wrap items-center gap-2 pt-1">
            <button
              type="button"
              disabled={!canAddToChat}
              onClick={() => onAddToChat(draft)}
              className="inline-flex items-center gap-1 rounded-md bg-content px-3 h-6.5 text-[12px] text-background-base hover:bg-content/80 disabled:cursor-default disabled:opacity-40"
            >
              Add to chat
            </button>
            <button
              type="button"
              onClick={() => {
                skipSave.current = true;
                if (saveTimer.current != null)
                  window.clearTimeout(saveTimer.current);
                void onDelete(note.id);
              }}
              className="inline-flex items-center gap-1.5 rounded-md px-3 h-7 text-[12px] text-content/70 hover:bg-content/10 hover:text-red-400"
            >
              <Trash2 className="size-3.5" strokeWidth={1.75} />
              Delete
            </button>
          </div>
          {saveError ? (
            <p className="text-[12px] text-red-400/90">{saveError}</p>
          ) : null}
        </header>
        <div
          role="tablist"
          aria-label="Note sections"
          className="flex h-9 items-stretch gap-4 border-b border-content/10"
        >
          <NoteDetailTab
            label="Preview"
            selected={mode === "preview"}
            onSelect={() => setMode("preview")}
          />
          <NoteDetailTab
            label="Source"
            selected={mode === "source"}
            onSelect={() => setMode("source")}
          />
        </div>
        <div
          ref={dropZoneRef}
          aria-busy={imageBusy}
          className={`relative min-h-[448px] rounded-lg border transition-colors ${
            imageDrag ? "border-accent/60 bg-accent/5" : "border-transparent"
          }`}
          onDragOver={(event: ReactDragEvent<HTMLDivElement>) => {
            if (!hasDroppedFiles(event.dataTransfer)) return;
            event.preventDefault();
            event.dataTransfer.dropEffect = "copy";
            setImageDrag(true);
          }}
          onDragLeave={(event: ReactDragEvent<HTMLDivElement>) => {
            const next = event.relatedTarget as Node | null;
            if (next && event.currentTarget.contains(next)) return;
            setImageDrag(false);
          }}
          onDrop={(event: ReactDragEvent<HTMLDivElement>) => {
            if (!hasDroppedFiles(event.dataTransfer)) return;
            event.preventDefault();
            setImageDrag(false);
            if (Date.now() - lastDropAt.current < 250) return;
            lastDropAt.current = Date.now();
            const files = [...event.dataTransfer.files];
            if (files.length === 0) return;
            const range = insertionRange();
            void addDroppedImages(
              () => saveNoteImagesFromFiles(note.id, files),
              range,
            );
          }}
        >
          {imageDrag || imageBusy ? (
            <div className="pointer-events-none absolute inset-0 z-20 grid place-items-center rounded-lg bg-background-base/80 text-[12px] text-content/70 backdrop-blur-sm">
              {imageBusy ? "Adding images…" : "Drop images here"}
            </div>
          ) : null}
          {mode === "source" ? (
            <NoteSource
              textareaRef={sourceFieldRef}
              autoFocus={blank}
              value={body}
              onChange={(next) => {
                setBody(next);
                scheduleSave();
              }}
            />
          ) : body.trim() ? (
            <AgentMarkdown text={body} cwd={note.sourceCwd} />
          ) : (
            <p className="text-[13px] text-content/45">No description</p>
          )}
        </div>
      </div>
    </div>
  );
}

function NoteSource({
  value,
  onChange,
  textareaRef,
  autoFocus = false,
}: {
  value: string;
  onChange: (value: string) => void;
  textareaRef: { current: HTMLTextAreaElement | null };
  autoFocus?: boolean;
}) {
  const lines = value.split("\n");
  const gutterWidth = `calc(${Math.max(String(lines.length).length, 2)}ch + 0.75rem)`;
  const textOffset = `calc(${gutterWidth} + 0.75rem)`;

  return (
    <div className="relative min-h-[448px]">
      <div
        aria-hidden
        className="pointer-events-none grid font-mono text-[13px] leading-5 text-content/85"
        style={{
          gridTemplateColumns: `${gutterWidth} minmax(0, 1fr)`,
        }}
      >
        {lines.map((line, index) => (
          <Fragment key={index}>
            <div className="select-none pr-2 text-right tabular-nums whitespace-nowrap text-content/40">
              {index + 1}
            </div>
            <div className="min-h-5 min-w-0 pl-3 whitespace-pre-wrap wrap-break-word">
              {line ? <MarkdownSourceHighlight text={line} /> : "\u00a0"}
            </div>
          </Fragment>
        ))}
      </div>
      <div
        aria-hidden
        className="pointer-events-none absolute inset-y-0 w-px bg-content/10"
        style={{ left: gutterWidth }}
      />
      <textarea
        ref={textareaRef}
        value={value}
        autoFocus={autoFocus}
        onChange={(event) => onChange(event.target.value)}
        spellCheck={false}
        placeholder="Write markdown…"
        className="markdown-source-field absolute inset-0 h-full w-full resize-none overflow-hidden border-0 bg-transparent py-0 pr-0 font-mono text-[13px] leading-5 whitespace-pre-wrap wrap-break-word outline-none"
        style={{ paddingLeft: textOffset }}
      />
    </div>
  );
}

function NoteTagsEditor({
  tags,
  onChange,
}: {
  tags: string[];
  onChange: (tags: string[]) => void;
}) {
  const [value, setValue] = useState("");

  const addTag = (input = value) => {
    const next = normalizeNoteTags([...tags, input]);
    setValue("");
    if (!sameTags(next, tags)) onChange(next);
  };

  return (
    <div
      className="flex min-w-0 flex-wrap items-center gap-1.5"
      aria-label="Tags"
    >
      <span className="mr-0.5 text-[11px] text-content/45">Tags</span>
      {tags.map((tag) => (
        <span
          key={tag}
          className="inline-flex h-6 max-w-48 items-center gap-1 rounded-md bg-content/8 pl-2 pr-1 text-[11px] text-content/70"
        >
          <span className="truncate">#{tag}</span>
          <button
            type="button"
            title={`Remove #${tag}`}
            aria-label={`Remove #${tag}`}
            onClick={() => onChange(tags.filter((item) => item !== tag))}
            className="grid size-4 shrink-0 place-items-center rounded text-content/40 hover:bg-content/10 hover:text-content"
          >
            <X className="size-2.5" strokeWidth={1.75} />
          </button>
        </span>
      ))}
      {tags.length < MAX_NOTE_TAGS ? (
        <input
          value={value}
          onChange={(event) => {
            const next = event.target.value;
            if (next.endsWith(",")) addTag(next.slice(0, -1));
            else setValue(next);
          }}
          onBlur={() => {
            if (value.trim()) addTag();
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === ",") {
              event.preventDefault();
              addTag();
              return;
            }
            if (event.key === "Backspace" && !value && tags.length > 0) {
              onChange(tags.slice(0, -1));
            }
          }}
          aria-label="Add note tag"
          placeholder="Add tag…"
          spellCheck={false}
          autoComplete="off"
          className="h-6 min-w-20 flex-1 border-0 bg-transparent px-1 text-[11px] text-content outline-none placeholder:text-content/35"
        />
      ) : null}
    </div>
  );
}

function sameTags(left: readonly string[], right: readonly string[]): boolean {
  return (
    left.length === right.length &&
    left.every((tag, index) => tag === right[index])
  );
}

function hasDroppedFiles(data: DataTransfer | null): data is DataTransfer {
  if (!data) return false;
  return [...data.types].some(
    (type) => type === "Files" || type === "application/x-moz-file",
  );
}

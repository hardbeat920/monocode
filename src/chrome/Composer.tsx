import { ArrowUp, Plus, Square, StickyNote } from "./icons";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type ClipboardEvent,
  type KeyboardEvent,
  type ReactNode,
  type UIEvent,
} from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  attachmentsFromFiles,
  attachmentsFromPaths,
  filesFromClipboard,
  mergeAttachments,
  pickAttachments,
  revokeAttachment,
} from "../lib/attachments";
import type { ContextUsage } from "../lib/contextUsage";
import {
  loadProjectFiles,
  peekProjectFiles,
  recentOpenedFiles,
} from "../lib/fileIndex";
import {
  buildMentionIndex,
  fileMentionParts,
  mentionLabel,
  mentionTokenAt,
  rankMentionFiles,
  replaceMentionToken,
  type MentionIndex,
  type MentionToken,
} from "../lib/fileMentions";
import type { ProjectFile } from "../lib/fs";
import {
  composeInboxMessage,
  type InboxComposerCard,
} from "../lib/githubTasks";
import { looksLikeProject, type RecentProject } from "../lib/recents";
import type { Attachment, HarnessId, RuntimeMode } from "../lib/session";
import { harnessSupportsAttachments } from "../lib/session";
import {
  createBlankSkill,
  rankSkills,
  replaceSlashToken,
  skillTextParts,
  slashTokenAt,
  type Skill,
  type SlashToken,
} from "../lib/skills";
import { AccessPicker } from "./AccessPicker";
import { ComposerRunner } from "./ComposerRunner";
import { ContextMeter } from "./ContextMeter";
import { AttachmentChip } from "./AttachmentChip";
import { BranchPicker } from "./BranchPicker";
import { CwdPicker } from "./CwdPicker";
import { FileMentionPicker } from "./FileMentionPicker";
import { FileTypeIcon } from "./FileTypeIcon";
import { InboxMiniCard } from "./InboxMiniCard";
import { NoteMiniCard } from "./NoteMiniCard";
import { ModelPicker } from "./ModelPicker";
import { ModelSettings } from "./ModelSettings";
import { SkillPicker } from "./SkillPicker";
import { projectName } from "../lib/paths";
import { consumeQuoteRequest, type QuoteRequest } from "../lib/quoteDraft";
import { useTabGroupLogos } from "../hooks/useTabGroupLogos";
import {
  COMPOSER_RUNNER_CHANGE_EVENT,
  loadComposerRunner,
  loadNotesEnabled,
  subscribeNotesEnabled,
} from "../lib/settings";
import {
  isNoteMentionPath,
  loadNotes,
  peekNotes,
  rankNoteFiles,
  notesAsProjectFiles,
  type Note,
  type NoteComposerCard,
} from "../lib/notes";
import { resolveTabGroupLogo } from "../lib/tabGroups";
import { useComposerSkills } from "./useComposerSkills";

type Props = {
  enabled?: boolean;
  focused: boolean;
  shell?: boolean;
  harness: HarnessId;
  model: string;
  modelSettings?: Record<string, string>;
  runtimeMode: RuntimeMode;
  cwd?: string;
  executionCwd: string;
  branch?: string;
  recents?: RecentProject[];
  hideProjectPicker?: boolean;
  context?: ContextUsage;
  quoteRequest?: QuoteRequest;
  initialDraft?: string;
  inboxCard?: InboxComposerCard;
  noteCard?: NoteComposerCard;
  busy?: boolean;
  hotkeys?: boolean;
  onFocus: () => void;
  onCwdChange: (cwd: string) => void;
  onBranchChange?: () => void;
  onNewTerminal?: () => void;
  onModelChange: (harness: HarnessId, model: string) => void;
  onModelSettingsChange?: (settings: Record<string, string>) => void;
  onRuntimeModeChange: (mode: RuntimeMode) => void;
  onQuoteRequestConsumed?: (id: number) => void;
  onInboxCardDismiss?: () => void;
  onNoteCardDismiss?: () => void;
  onSubmit: (text: string, attachments: Attachment[]) => void;
  onStop?: () => void;
  onOpenFile?: (path: string) => void;
  children?: ReactNode;
};

function ToolButton({
  active,
  disabled,
  label,
  onClick,
  children,
}: {
  active?: boolean;
  disabled?: boolean;
  label: string;
  onClick?: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      className={`grid size-6.5 shrink-0 place-items-center rounded-md ${
        active
          ? "bg-content/20 text-content"
          : "bg-content/10 text-content/50 hover:bg-content/15 hover:text-content"
      } disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-content/50`}
    >
      {children}
    </button>
  );
}

export function Composer({
  enabled = true,
  focused,
  hotkeys = false,
  shell = false,
  harness,
  model,
  modelSettings = {},
  runtimeMode,
  cwd = "~",
  executionCwd,
  branch,
  recents = [],
  hideProjectPicker = false,
  context,
  quoteRequest,
  initialDraft,
  inboxCard,
  noteCard,
  busy = false,
  onFocus,
  onCwdChange,
  onBranchChange,
  onNewTerminal,
  onModelChange,
  onModelSettingsChange,
  onRuntimeModeChange,
  onQuoteRequestConsumed,
  onInboxCardDismiss,
  onNoteCardDismiss,
  onSubmit,
  onStop,
  onOpenFile,
  children,
}: Props) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const boxRef = useRef<HTMLDivElement>(null);
  const highlightRef = useRef<HTMLDivElement>(null);
  const attachmentsRef = useRef<Attachment[]>([]);
  const consumedQuoteId = useRef<number | null>(null);
  const slashRef = useRef<SlashToken | null>(null);
  const mentionRef = useRef<MentionToken | null>(null);
  const [draft, setDraft] = useState(initialDraft ?? "");
  const [hasValue, setHasValue] = useState(
    () => (initialDraft ?? "").trim().length > 0 || !!inboxCard || !!noteCard,
  );
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [fileDrag, setFileDrag] = useState(false);
  const [slash, setSlash] = useState<SlashToken | null>(null);
  const [skillActive, setSkillActive] = useState(0);
  const [creatingSkill, setCreatingSkill] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [createBusy, setCreateBusy] = useState(false);
  const [files, setFiles] = useState<ProjectFile[]>(
    () => peekProjectFiles(cwd) ?? [],
  );
  const notesEnabled = useSyncExternalStore(
    subscribeNotesEnabled,
    loadNotesEnabled,
    () => true,
  );
  const [notes, setNotes] = useState<Note[]>(() => peekNotes() ?? []);
  const [mention, setMention] = useState<MentionToken | null>(null);
  const [mentionActive, setMentionActive] = useState(0);
  const [runnerEnabled, setRunnerEnabled] = useState(loadComposerRunner);
  const [runnerLive, setRunnerLive] = useState(
    () => busy && loadComposerRunner(),
  );
  const groupLogos = useTabGroupLogos();
  const projectLogoPath = resolveTabGroupLogo(projectName(cwd), groupLogos);

  slashRef.current = slash;
  mentionRef.current = mention;

  attachmentsRef.current = attachments;

  const mentionOpen =
    mention !== null && (looksLikeProject(cwd) || notesEnabled);
  const pickerOpen = creatingSkill || slash !== null;
  const skillCatalog = useComposerSkills({
    harness,
    executionCwd,
    pickerOpen,
  });
  const skills = skillCatalog.skills;
  const skillLimit =
    harness === "pi" ? Number.POSITIVE_INFINITY : undefined;
  const rankedSkills = rankSkills(
    skills,
    slash?.query ?? "",
    skillLimit,
  );
  const attachmentsSupported = harnessSupportsAttachments(harness);
  const skillNames = useMemo(
    () => new Set(skills.map((skill) => skill.invocation)),
    [skills],
  );
  const mentionFiles = useMemo(
    () =>
      notesEnabled ? [...files, ...notesAsProjectFiles(notes)] : files,
    [files, notes, notesEnabled],
  );
  const mentionIndex = useMemo(
    () => buildMentionIndex(mentionFiles),
    [mentionFiles],
  );
  const mentionIndexRef = useRef<MentionIndex>(mentionIndex);
  mentionIndexRef.current = mentionIndex;
  const rankedFiles = useMemo(() => {
    if (!mentionOpen) return [];
    const fileHits = looksLikeProject(cwd)
      ? rankMentionFiles(files, mention?.query ?? "", recentOpenedFiles(cwd))
      : [];
    const noteHits = notesEnabled
      ? rankNoteFiles(notes, mention?.query ?? "")
      : [];
    const seen = new Set(noteHits.map((file) => file.path));
    return [...noteHits, ...fileHits.filter((file) => !seen.has(file.path))];
  }, [cwd, files, mention?.query, mentionOpen, notes, notesEnabled]);

  const syncHasValue = useCallback(
    (text: string, files: Attachment[]) => {
      setHasValue(
        text.trim().length > 0 || files.length > 0 || !!inboxCard || !!noteCard,
      );
    },
    [inboxCard, noteCard],
  );

  useEffect(() => {
    syncHasValue(ref.current?.value ?? "", attachmentsRef.current);
  }, [inboxCard, noteCard, syncHasValue]);

  const addAttachments = useCallback(
    (incoming: Attachment[]) => {
      if (!harnessSupportsAttachments(harness) || incoming.length === 0) return;
      setAttachments((prev) => {
        const next = mergeAttachments(prev, incoming);
        syncHasValue(ref.current?.value ?? "", next);
        return next;
      });
      ref.current?.focus();
    },
    [harness, syncHasValue],
  );

  const removeAttachment = useCallback(
    (id: string) => {
      setAttachments((prev) => {
        const removed = prev.find((file) => file.id === id);
        if (removed) revokeAttachment(removed);
        const next = prev.filter((file) => file.id !== id);
        syncHasValue(ref.current?.value ?? "", next);
        return next;
      });
      ref.current?.focus();
    },
    [syncHasValue],
  );

  useEffect(() => {
    return () => {
      for (const file of attachmentsRef.current) revokeAttachment(file);
    };
  }, []);

  useEffect(() => {
    if (harnessSupportsAttachments(harness)) return;
    setAttachments((prev) => {
      if (prev.length === 0) return prev;
      for (const file of prev) revokeAttachment(file);
      syncHasValue(ref.current?.value ?? "", []);
      return [];
    });
  }, [harness, syncHasValue]);

  useEffect(() => {
    const refresh = () => setRunnerEnabled(loadComposerRunner());
    window.addEventListener(COMPOSER_RUNNER_CHANGE_EVENT, refresh);
    return () =>
      window.removeEventListener(COMPOSER_RUNNER_CHANGE_EVENT, refresh);
  }, []);

  useEffect(() => {
    if (!runnerEnabled) {
      setRunnerLive(false);
      return;
    }
    if (busy) setRunnerLive(true);
  }, [busy, runnerEnabled]);

  useEffect(() => {
    setSkillActive(0);
  }, [slash?.query, cwd]);

  useEffect(() => {
    setSkillActive((index) =>
      rankedSkills.length === 0 ? 0 : Math.min(index, rankedSkills.length - 1),
    );
  }, [rankedSkills.length]);

  useEffect(() => {
    let cancelled = false;
    void loadProjectFiles(cwd, mentionOpen)
      .then((next) => {
        if (!cancelled) setFiles(next);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [cwd, mentionOpen]);

  useEffect(() => {
    if (!mentionOpen || !notesEnabled) return;
    let cancelled = false;
    void loadNotes().then((next) => {
      if (!cancelled) setNotes(next);
    });
    return () => {
      cancelled = true;
    };
  }, [mentionOpen, notesEnabled]);

  useEffect(() => {
    setMentionActive(0);
  }, [mention?.query, cwd]);

  useEffect(() => {
    setMentionActive((index) =>
      rankedFiles.length === 0 ? 0 : Math.min(index, rankedFiles.length - 1),
    );
  }, [rankedFiles.length]);

  const resizeTextarea = (el: HTMLTextAreaElement) => {
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
  };

  useEffect(() => {
    const el = ref.current;
    if (!el || !initialDraft) return;
    el.value = initialDraft;
    resizeTextarea(el);
  }, [initialDraft]);

  const syncHighlightScroll = (e: UIEvent<HTMLTextAreaElement>) => {
    const highlight = highlightRef.current;
    if (!highlight) return;
    highlight.scrollTop = e.currentTarget.scrollTop;
    highlight.scrollLeft = e.currentTarget.scrollLeft;
  };

  const syncTokensFromTextarea = (el: HTMLTextAreaElement) => {
    if (creatingSkill) return;
    const cursor = el.selectionStart ?? 0;
    const token = slashTokenAt(el.value, cursor);
    setSlash(token);
    setMention(token ? null : mentionTokenAt(el.value, cursor));
  };

  useEffect(() => {
    const el = ref.current;
    if (!el || !quoteRequest) return;

    const result = consumeQuoteRequest(
      el.value,
      consumedQuoteId.current,
      quoteRequest,
    );
    consumedQuoteId.current = result.consumedId;
    if (result.changed) {
      el.value = result.draft;
      resizeTextarea(el);
      setDraft(result.draft);
      syncHasValue(result.draft, attachmentsRef.current);
      setSlash(null);
      setMention(null);
      setCreatingSkill(false);
      setCreateError(null);
      el.setSelectionRange(result.draft.length, result.draft.length);
      el.focus();
    }
    onQuoteRequestConsumed?.(quoteRequest.id);
  }, [onQuoteRequestConsumed, quoteRequest, syncHasValue]);

  const pickSkill = useCallback(
    (skill: Skill) => {
      const el = ref.current;
      const token = slashRef.current;
      if (!el || !token) {
        setSlash(null);
        setCreatingSkill(false);
        return;
      }
      const next = replaceSlashToken(el.value, token, skill.invocation);
      el.value = next;
      resizeTextarea(el);
      let cursor = token.start + skill.invocation.length + 1;
      if (next[cursor] === " ") cursor += 1;
      el.setSelectionRange(cursor, cursor);
      setDraft(next);
      syncHasValue(next, attachmentsRef.current);
      setSlash(null);
      setCreatingSkill(false);
      el.focus();
    },
    [syncHasValue],
  );

  const pickMention = useCallback(
    (file: ProjectFile) => {
      const el = ref.current;
      const token = mentionRef.current;
      if (!el || !token) {
        setMention(null);
        return;
      }
      const label = isNoteMentionPath(file.path)
        ? file.relative
        : mentionLabel(file, mentionIndexRef.current);
      const next = replaceMentionToken(el.value, token, label);
      el.value = next;
      resizeTextarea(el);
      let cursor = token.start + label.length + 1;
      if (next[cursor] === " ") cursor += 1;
      el.setSelectionRange(cursor, cursor);
      setDraft(next);
      syncHasValue(next, attachmentsRef.current);
      setMention(null);
      el.focus();
    },
    [syncHasValue],
  );

  useEffect(() => {
    if (!focused) return;
    if (
      document.querySelector(
        "[data-model-picker], [data-access-picker], [data-model-settings], [data-file-picker], [data-branch-picker], [data-skill-picker], [data-mention-picker]",
      )
    )
      return;
    ref.current?.focus();
  }, [focused]);

  useEffect(() => {
    if (!enabled) {
      setFileDrag(false);
      return;
    }
    const dropRoot = () =>
      boxRef.current?.closest("[data-session-drop]") as HTMLElement | null;
    let nativeDropAt = 0;

    const toClientPoint = (x: number, y: number) => {
      const scale = window.devicePixelRatio || 1;
      // Tauri types this as PhysicalPosition, but macOS wry reports logical
      // points. Only scale down when the point sits outside the CSS viewport.
      if (scale !== 1 && (x > window.innerWidth || y > window.innerHeight)) {
        return { x: x / scale, y: y / scale };
      }
      return { x, y };
    };

    const overTarget = (x: number, y: number) => {
      const root = dropRoot();
      if (!root) return false;
      const point = toClientPoint(x, y);
      const rect = root.getBoundingClientRect();
      return (
        point.x >= rect.left &&
        point.x <= rect.right &&
        point.y >= rect.top &&
        point.y <= rect.bottom
      );
    };

    const onDragOver = (event: DragEvent) => {
      const data = event.dataTransfer;
      if (!hasFiles(data)) return;
      event.preventDefault();
      if (!attachmentsSupported) return;
      data.dropEffect = "copy";
      setFileDrag(true);
    };
    const onDragLeave = (event: DragEvent) => {
      const root = dropRoot();
      if (!root) return;
      const next = event.relatedTarget as Node | null;
      if (next && root.contains(next)) return;
      setFileDrag(false);
    };
    const onDrop = (event: DragEvent) => {
      const data = event.dataTransfer;
      if (!hasFiles(data)) return;
      event.preventDefault();
      setFileDrag(false);
      if (!attachmentsSupported) return;
      if (Date.now() - nativeDropAt < 250) return;
      const files = [...data.files];
      if (files.length === 0) return;
      void attachmentsFromFiles(files).then(addAttachments);
    };

    const root = dropRoot();
    root?.addEventListener("dragover", onDragOver);
    root?.addEventListener("dragleave", onDragLeave);
    root?.addEventListener("drop", onDrop);

    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "leave") {
          setFileDrag(false);
          return;
        }
        const { x, y } = event.payload.position;
        const over = overTarget(x, y);
        if (event.payload.type === "enter" || event.payload.type === "over") {
          setFileDrag(over && attachmentsSupported);
          return;
        }
        if (event.payload.type !== "drop") return;
        setFileDrag(false);
        if (!over || !attachmentsSupported) return;
        nativeDropAt = Date.now();
        void attachmentsFromPaths(event.payload.paths).then(addAttachments);
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => undefined);

    return () => {
      cancelled = true;
      root?.removeEventListener("dragover", onDragOver);
      root?.removeEventListener("dragleave", onDragLeave);
      root?.removeEventListener("drop", onDrop);
      unlisten?.();
    };
  }, [addAttachments, attachmentsSupported, enabled]);

  const submit = (value: string) => {
    const text = composeInboxMessage(inboxCard, value);
    const files = attachments;
    if (!text && files.length === 0 && !noteCard) return;
    onSubmit(text, files);
    if (!ref.current) return;
    ref.current.value = "";
    ref.current.style.height = "auto";
    setDraft("");
    setAttachments([]);
    setSlash(null);
    setMention(null);
    setCreatingSkill(false);
    setCreateError(null);
    syncHasValue("", []);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (creatingSkill) return;

    if (mentionOpen) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        if (rankedFiles.length === 0) return;
        setMentionActive((index) => (index + 1) % rankedFiles.length);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        if (rankedFiles.length === 0) return;
        setMentionActive(
          (index) => (index - 1 + rankedFiles.length) % rankedFiles.length,
        );
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setMention(null);
        return;
      }
      if (e.key === "Tab") {
        e.preventDefault();
        const file = rankedFiles[mentionActive];
        if (file) pickMention(file);
        return;
      }
      if (e.key === "Enter" && !e.shiftKey) {
        const file = rankedFiles[mentionActive];
        if (file) {
          e.preventDefault();
          pickMention(file);
          return;
        }
        setMention(null);
      }
    }

    if (slash) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        if (rankedSkills.length === 0) return;
        setSkillActive((index) => (index + 1) % rankedSkills.length);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        if (rankedSkills.length === 0) return;
        setSkillActive(
          (index) => (index - 1 + rankedSkills.length) % rankedSkills.length,
        );
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setSlash(null);
        return;
      }
      if (e.key === "Tab") {
        e.preventDefault();
        const skill = rankedSkills[skillActive];
        if (skill) pickSkill(skill);
        return;
      }
      if (e.key === "Enter" && !e.shiftKey) {
        const skill = rankedSkills[skillActive];
        if (skill) {
          e.preventDefault();
          pickSkill(skill);
          return;
        }
        if (!slash.query) {
          e.preventDefault();
          return;
        }
        setSlash(null);
      }
    }

    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit(e.currentTarget.value);
    }
  };

  const onPaste = (e: ClipboardEvent<HTMLTextAreaElement>) => {
    const files = filesFromClipboard(e.clipboardData);
    if (files.length === 0) return;
    e.preventDefault();
    if (!attachmentsSupported) return;
    void attachmentsFromFiles(files).then(addAttachments);
  };

  const attachFromPicker = () => {
    if (!attachmentsSupported) return;
    void pickAttachments().then((files) => {
      addAttachments(files);
      ref.current?.focus();
    });
  };

  return (
    <div
      data-composer
      className={`relative shrink-0 ${shell ? "" : "p-1.5 pt-0"}`}
      onMouseDown={onFocus}
    >
      {children}
      <div className="relative overflow-visible">
        {pickerOpen ? (
          <div className="absolute inset-x-0 bottom-full z-30 mb-1">
            <SkillPicker
              skills={rankedSkills}
              query={slash?.query ?? ""}
              active={skillActive}
              creating={creatingSkill}
              cwd={cwd}
              error={createError}
              busy={createBusy}
              onActive={setSkillActive}
              onPick={pickSkill}
              onStartCreate={() => {
                setCreatingSkill(true);
                setCreateError(null);
              }}
              onCancelCreate={() => {
                setCreatingSkill(false);
                setCreateError(null);
                const el = ref.current;
                if (el) syncTokensFromTextarea(el);
                el?.focus();
              }}
              onCreate={(name, scope) => {
                setCreateBusy(true);
                setCreateError(null);
                void createBlankSkill({ cwd, name, scope })
                  .then((path) => {
                    const el = ref.current;
                    const token = slashRef.current;
                    if (el && token) {
                      const rest = el.value.slice(token.end).replace(/^\s/, "");
                      const next = `${el.value.slice(0, token.start)}${rest}`;
                      el.value = next;
                      resizeTextarea(el);
                      el.setSelectionRange(token.start, token.start);
                      setDraft(next);
                      syncHasValue(next, attachments);
                    }
                    setCreatingSkill(false);
                    setSlash(null);
                    setCreateError(null);
                    void skillCatalog
                      .refresh({ refresh: true })
                      .catch(() => undefined);
                    onOpenFile?.(path);
                    el?.focus();
                  })
                  .catch((err: unknown) => {
                    setCreateError(
                      err instanceof Error ? err.message : String(err),
                    );
                  })
                  .finally(() => setCreateBusy(false));
              }}
            />
          </div>
        ) : null}
        {mentionOpen && !pickerOpen ? (
          <div className="absolute inset-x-0 bottom-full z-30 mb-1">
            <FileMentionPicker
              files={rankedFiles}
              query={mention?.query ?? ""}
              active={mentionActive}
              loading={
                looksLikeProject(cwd) && peekProjectFiles(cwd) == null
              }
              includeNotes={notesEnabled}
              onActive={setMentionActive}
              onPick={pickMention}
            />
          </div>
        ) : null}
        <div
          ref={boxRef}
          data-composer-box
          className={`relative z-10 rounded-lg border bg-content/3 ${
            fileDrag
              ? "border-accent/60"
              : "border-content/10 has-focus:border-content/20"
          }`}
        >
          {fileDrag ? (
            <div className="pointer-events-none absolute inset-0 z-20 grid place-items-center rounded-lg bg-accent/8 text-[12px] text-content/70">
              Drop files to attach
            </div>
          ) : null}
          <div className="flex min-w-0 items-center gap-2.5 px-3 pt-2.5">
            {hideProjectPicker ? null : (
              <CwdPicker
                cwd={cwd}
                recents={recents}
                projectLogoPath={projectLogoPath}
                enabled={enabled}
                onCwdChange={onCwdChange}
                onNewTerminal={onNewTerminal}
                onClose={() => ref.current?.focus()}
              />
            )}
            <BranchPicker
              cwd={cwd}
              branch={branch}
              enabled={enabled && !busy}
              onChange={onBranchChange}
              onClose={() => ref.current?.focus()}
            />
            <div className="ml-auto flex shrink-0 items-center">
              <ContextMeter usage={context} />
            </div>
          </div>

          {attachments.length > 0 ? (
            <div className="flex flex-wrap gap-1.5 px-3 pt-2">
              {attachments.map((file) => (
                <AttachmentChip
                  key={file.id}
                  attachment={file}
                  onRemove={() => removeAttachment(file.id)}
                />
              ))}
            </div>
          ) : null}

          {inboxCard ? (
            <InboxMiniCard card={inboxCard} onDismiss={onInboxCardDismiss} />
          ) : null}

          {noteCard ? (
            <NoteMiniCard card={noteCard} onDismiss={onNoteCardDismiss} />
          ) : null}

          <div className="relative">
            <div
              ref={highlightRef}
              aria-hidden
              className={`composer-highlight pointer-events-none absolute inset-0 max-h-40 overflow-hidden whitespace-pre-wrap break-words px-3 text-sm leading-5.5 text-content font-sans ${
                shell ? "py-4" : "py-3"
              }`}
            >
              <ComposerHighlight
                text={draft}
                names={skillNames}
                mentions={mentionIndex.labels}
              />
            </div>
            <textarea
              ref={ref}
              rows={1}
              spellCheck={false}
              defaultValue={initialDraft}
              placeholder={
                inboxCard
                  ? "Add a note, or send to start…"
                  : noteCard
                    ? "Add a message, or send…"
                    : shell
                      ? "How can I help you today?"
                      : "Ask, build, / for skills, @ for references... "
              }
              className={`composer-field relative max-h-40 w-full resize-none overflow-x-hidden whitespace-pre-wrap break-words bg-transparent px-3 text-sm leading-5.5 outline-none placeholder:overflow-hidden placeholder:text-ellipsis placeholder:whitespace-nowrap font-sans ${
                shell ? "py-4" : "py-3"
              }`}
              onFocus={onFocus}
              onKeyDown={onKeyDown}
              onPaste={onPaste}
              onScroll={syncHighlightScroll}
              onClick={(e) => syncTokensFromTextarea(e.currentTarget)}
              onKeyUp={(e) => syncTokensFromTextarea(e.currentTarget)}
              onSelect={(e) => syncTokensFromTextarea(e.currentTarget)}
              onInput={(e) => {
                const el = e.currentTarget;
                resizeTextarea(el);
                setDraft(el.value);
                syncHasValue(el.value, attachments);
                syncTokensFromTextarea(el);
              }}
            />
          </div>

          <div className="flex items-center gap-1 px-2 pb-2">
            <ToolButton
              label={
                attachmentsSupported
                  ? "Attach files"
                  : "fx does not support attachments"
              }
              disabled={!attachmentsSupported}
              onClick={attachFromPicker}
            >
              <Plus className="size-3.5" strokeWidth={1.5} />
            </ToolButton>
            <div
              className="composer-toolbar flex min-w-0 flex-1 items-center"
              onWheel={(e) => {
                if (
                  e.target instanceof Element &&
                  e.target.closest(
                    "[data-model-picker], [data-access-picker], [data-model-settings]",
                  )
                ) {
                  return;
                }
                const el = e.currentTarget;
                if (el.scrollWidth <= el.clientWidth) return;
                if (e.deltaX === 0 && e.deltaY !== 0) el.scrollLeft += e.deltaY;
              }}
            >
              <div className="flex shrink-0 items-center gap-1">
                <ModelPicker
                  harness={harness}
                  model={model}
                  hotkeys={hotkeys && enabled}
                  onChange={onModelChange}
                  onClose={() => ref.current?.focus()}
                />
                <ModelSettings
                  harness={harness}
                  model={model}
                  values={modelSettings}
                  onChange={(settings) => onModelSettingsChange?.(settings)}
                  onClose={() => ref.current?.focus()}
                />
                {harness !== "fx" ? (
                  <AccessPicker
                    value={runtimeMode}
                    onChange={onRuntimeModeChange}
                    onClose={() => ref.current?.focus()}
                  />
                ) : null}
              </div>
            </div>

            <div className="flex shrink-0 items-center gap-1">
              <ComposerAction
                busy={busy}
                hasValue={hasValue}
                onSend={() => submit(ref.current?.value ?? "")}
                onStop={() => onStop?.()}
              />
            </div>
          </div>
        </div>
        {runnerLive && runnerEnabled ? (
          <ComposerRunner
            boxRef={boxRef}
            cwd={cwd}
            busy={busy}
            enabled={enabled}
            onExited={() => setRunnerLive(false)}
          />
        ) : null}
      </div>
    </div>
  );
}

function ComposerHighlight({
  text,
  names,
  mentions,
}: {
  text: string;
  names: ReadonlySet<string>;
  mentions: ReadonlyMap<string, ProjectFile>;
}) {
  const parts = skillTextParts(text, names);
  return (
    <>
      {parts.map((part, index) =>
        part.skill ? (
          <span key={index} className="text-skill">
            {part.text}
          </span>
        ) : (
          // Skill tokens always end on whitespace, so each remaining run still
          // starts on a boundary `@mention` matching can rely on.
          <MentionRuns key={index} text={part.text} mentions={mentions} />
        ),
      )}
      {text.endsWith("\n") ? "\n" : null}
    </>
  );
}

function MentionRuns({
  text,
  mentions,
}: {
  text: string;
  mentions: ReadonlyMap<string, ProjectFile>;
}) {
  const parts = fileMentionParts(text, mentions);
  return (
    <>
      {parts.map((part, index) =>
        part.file ? (
          <span key={index} className="text-mention">
            {/* The `@` keeps its width so the textarea underneath stays in
                lockstep; the file icon sits on top of it. */}
            <span className="relative text-transparent">
              {"@"}
              <span className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2">
                {part.file && isNoteMentionPath(part.file.path) ? (
                  <StickyNote className="size-3.5" strokeWidth={1.75} />
                ) : (
                  <FileTypeIcon
                    name={part.file.name}
                    isDir={Boolean(part.file.isDir)}
                    size={13}
                  />
                )}
              </span>
            </span>
            {part.text.slice(1)}
          </span>
        ) : (
          part.text
        ),
      )}
    </>
  );
}

function ComposerAction({
  busy,
  hasValue,
  onSend,
  onStop,
}: {
  busy: boolean;
  hasValue: boolean;
  onSend: () => void;
  onStop: () => void;
}) {
  if (busy) {
    return (
      <>
        {hasValue ? (
          <button
            type="button"
            title="Send"
            aria-label="Send"
            onClick={onSend}
            className="grid size-6.5 place-items-center rounded-md bg-white text-black hover:bg-white/90"
          >
            <ArrowUp className="size-3.5" strokeWidth={2.25} />
          </button>
        ) : null}
        <button
          type="button"
          title="Stop"
          aria-label="Stop"
          onClick={onStop}
          className="grid size-6.5 place-items-center rounded-md bg-white text-black hover:bg-white/90"
        >
          <Square className="size-2.5 fill-current" strokeWidth={0} />
        </button>
      </>
    );
  }

  return (
    <button
      type="button"
      title="Send"
      aria-label="Send"
      disabled={!hasValue}
      onClick={onSend}
      className="grid size-6.5 place-items-center rounded-md bg-white text-black hover:bg-white/90 disabled:cursor-default disabled:bg-white/30 disabled:text-black/40 disabled:hover:bg-white/30"
    >
      <ArrowUp className="size-3.5" strokeWidth={2.25} />
    </button>
  );
}

function hasFiles(data: DataTransfer | null): data is DataTransfer {
  if (!data) return false;
  return [...data.types].some(
    (type) => type === "Files" || type === "application/x-moz-file",
  );
}

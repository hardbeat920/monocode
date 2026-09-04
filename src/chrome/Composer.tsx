import {
  ArrowUp,
  AiIdea,
  Check,
  CornerDownRight,
  FilePlus,
  ListEnd,
  Pause,
  Pencil,
  Play,
  Plus,
  Square,
  StickyNote,
  Trash2,
  X,
} from "./icons";
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
  subscribeProjectFiles,
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
import type { HandoffComposerCard } from "../lib/handoff";
import { looksLikeProject, type RecentProject } from "../lib/recents";
import type {
  Attachment,
  HarnessId,
  MessageQueueStatus,
  QueuedMessage,
  RuntimeMode,
  TurnIntent,
} from "../lib/session";
import { HARNESS_TITLE, harnessSupportsAttachments } from "../lib/session";
import type {
  UserQuestionPrompt,
  UserQuestionReply,
} from "../lib/userQuestion";
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
import { HandoffMiniCard } from "./HandoffMiniCard";
import { ModelPicker } from "./ModelPicker";
import { ModelSettings } from "./ModelSettings";
import { QuestionForm } from "./QuestionForm";
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
import { Popover } from "./Popover";
import { consumePlanCommand, PLAN_COMMAND } from "../lib/plan";
import { COMPACT_COMMAND, isCompactCommand } from "../lib/compact";

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
  compactSupported?: boolean;
  quoteRequest?: QuoteRequest;
  initialDraft?: string;
  inboxCard?: InboxComposerCard;
  noteCard?: NoteComposerCard;
  handoffCard?: HandoffComposerCard;
  question?: UserQuestionPrompt;
  busy?: boolean;
  queuedMessages?: QueuedMessage[];
  queueStatus?: MessageQueueStatus;
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
  onHandoffCardDismiss?: () => void;
  onQuestionReply?: (requestId: number, reply: UserQuestionReply) => void;
  onSubmit: (
    text: string,
    attachments: Attachment[],
    options?: { intent?: TurnIntent },
  ) => void;
  onStop?: () => void;
  onCompactContext?: () => boolean;
  onDeleteQueuedMessage?: (messageId: string) => void;
  onEditQueuedMessage?: (messageId: string, text: string) => void;
  onQueuedMessageEditingChange?: (messageId?: string) => void;
  onSteerQueuedMessage?: (messageId: string) => void;
  onResumeQueue?: () => void;
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

function MessageQueue({
  messages,
  status,
  onDelete,
  onEdit,
  onEditingChange,
  onSteer,
  onResume,
}: {
  messages: QueuedMessage[];
  status?: MessageQueueStatus;
  onDelete?: (messageId: string) => void;
  onEdit?: (messageId: string, text: string) => void;
  onEditingChange?: (messageId?: string) => void;
  onSteer?: (messageId: string) => void;
  onResume?: () => void;
}) {
  const [editingId, setEditingId] = useState<string>();
  const [editDraft, setEditDraft] = useState("");
  const onEditingChangeRef = useRef(onEditingChange);
  onEditingChangeRef.current = onEditingChange;
  const editingIdRef = useRef(editingId);
  editingIdRef.current = editingId;
  useEffect(() => {
    return () => {
      if (editingIdRef.current) onEditingChangeRef.current?.();
    };
  }, []);
  if (messages.length === 0) return null;
  const paused = status === "paused";

  const startEdit = (message: QueuedMessage) => {
    setEditingId(message.id);
    setEditDraft(message.text);
    onEditingChange?.(message.id);
  };
  const cancelEdit = () => {
    setEditingId(undefined);
    setEditDraft("");
    onEditingChange?.();
  };
  const saveEdit = (message: QueuedMessage) => {
    if (!editDraft.trim() && message.attachments.length === 0) return;
    onEdit?.(message.id, editDraft);
    setEditingId(undefined);
    setEditDraft("");
  };

  return (
    <div className="px-2 text-content/55" data-message-queue>
      <div
        className="relative z-0 rounded-t-[10px] border border-b-0 border-content/10 bg-content/3 px-2 py-1"
        data-message-queue-card
      >
        {paused ? (
          <div className="flex h-7 items-center gap-2 border-b border-content/10 text-[12px]">
            <Pause className="size-3.5" />
            <span className="min-w-0 flex-1 truncate">
              Queue paused because you interrupted
            </span>
            <button
              type="button"
              onClick={onResume}
              className="flex h-6 shrink-0 items-center gap-1.5 rounded-md px-1.5 hover:bg-content/10 hover:text-content"
            >
              <Play className="size-3.5" />
              Resume
            </button>
          </div>
        ) : null}
        {messages.map((message, index) => {
          const editing = editingId === message.id;
          const label =
            message.text.trim() ||
            `${message.attachments.length} attachment${message.attachments.length === 1 ? "" : "s"}`;
          return (
            <div
              key={message.id}
              className={`flex min-h-7 items-center gap-2 text-[12px] ${
                index > 0 ? "border-t border-content/10" : ""
              }`}
            >
              <ListEnd className="size-3.5 shrink-0" />
              {editing ? (
                <>
                  <textarea
                    autoFocus
                    aria-label="Edit queued message"
                    value={editDraft}
                    rows={1}
                    onChange={(event) => setEditDraft(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Escape") {
                        event.preventDefault();
                        cancelEdit();
                      } else if (event.key === "Enter" && !event.shiftKey) {
                        event.preventDefault();
                        saveEdit(message);
                      }
                    }}
                    className="min-h-6 min-w-0 flex-1 resize-none rounded-md border border-content/15 bg-content/5 px-1.5 py-0.5 text-[12px] text-content outline-none focus:border-content/30"
                  />
                  <button
                    type="button"
                    title="Save queued message"
                    aria-label="Save queued message"
                    disabled={
                      !editDraft.trim() && message.attachments.length === 0
                    }
                    onClick={() => saveEdit(message)}
                    className="grid size-6 shrink-0 place-items-center rounded-md hover:bg-content/10 hover:text-content disabled:opacity-30"
                  >
                    <Check className="size-3.5" />
                  </button>
                  <button
                    type="button"
                    title="Cancel queued message edit"
                    aria-label="Cancel queued message edit"
                    onClick={cancelEdit}
                    className="grid size-6 shrink-0 place-items-center rounded-md hover:bg-content/10 hover:text-content"
                  >
                    <X className="size-3.5" />
                  </button>
                </>
              ) : (
                <>
                  <span className="min-w-0 flex-1 truncate text-content/80">
                    {label}
                  </span>
                  <button
                    type="button"
                    onClick={() => onSteer?.(message.id)}
                    className="flex h-6 shrink-0 items-center gap-1.5 rounded-md px-1.5 hover:bg-content/10 hover:text-content"
                  >
                    <CornerDownRight className="size-3.5" />
                    Steer
                  </button>
                  <button
                    type="button"
                    title="Edit queued message"
                    aria-label="Edit queued message"
                    onClick={() => startEdit(message)}
                    className="grid size-6 shrink-0 place-items-center rounded-md hover:bg-content/10 hover:text-content"
                  >
                    <Pencil className="size-3.5" />
                  </button>
                  <button
                    type="button"
                    title="Remove queued message"
                    aria-label="Remove queued message"
                    onClick={() => onDelete?.(message.id)}
                    className="grid size-6 shrink-0 place-items-center rounded-md hover:bg-content/10 hover:text-content"
                  >
                    <Trash2 className="size-3.5" />
                  </button>
                </>
              )}
            </div>
          );
        })}
      </div>
    </div>
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
  compactSupported = false,
  quoteRequest,
  initialDraft,
  inboxCard,
  noteCard,
  handoffCard,
  question,
  busy = false,
  queuedMessages = [],
  queueStatus,
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
  onHandoffCardDismiss,
  onQuestionReply,
  onSubmit,
  onStop,
  onCompactContext,
  onDeleteQueuedMessage,
  onEditQueuedMessage,
  onQueuedMessageEditingChange,
  onSteerQueuedMessage,
  onResumeQueue,
  onOpenFile,
  children,
}: Props) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const boxRef = useRef<HTMLDivElement>(null);
  const plusRef = useRef<HTMLDivElement>(null);
  const highlightRef = useRef<HTMLDivElement>(null);
  const attachmentsRef = useRef<Attachment[]>([]);
  const consumedQuoteId = useRef<number | null>(null);
  const slashRef = useRef<SlashToken | null>(null);
  const mentionRef = useRef<MentionToken | null>(null);
  const [draft, setDraft] = useState(initialDraft ?? "");
  const [hasValue, setHasValue] = useState(
    () =>
      (initialDraft ?? "").trim().length > 0 ||
      !!inboxCard ||
      !!noteCard ||
      !!handoffCard,
  );
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [fileDrag, setFileDrag] = useState(false);
  const [plusOpen, setPlusOpen] = useState(false);
  const [planSelected, setPlanSelected] = useState(false);
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
  const slashItems = useMemo(
    () => [
      PLAN_COMMAND,
      COMPACT_COMMAND,
      ...skills.filter(
        (skill) =>
          skill.name !== PLAN_COMMAND.name &&
          skill.name !== COMPACT_COMMAND.name,
      ),
    ],
    [skills],
  );
  const skillLimit = harness === "pi" ? Number.POSITIVE_INFINITY : undefined;
  const rankedSkills = rankSkills(slashItems, slash?.query ?? "", skillLimit);
  const attachmentsSupported = harnessSupportsAttachments(harness);
  const skillNames = useMemo(
    () => new Set(slashItems.map((skill) => skill.invocation)),
    [slashItems],
  );
  const mentionFiles = useMemo(
    () => (notesEnabled ? [...files, ...notesAsProjectFiles(notes)] : files),
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
        text.trim().length > 0 ||
          files.length > 0 ||
          !!inboxCard ||
          !!noteCard ||
          !!handoffCard,
      );
    },
    [inboxCard, noteCard, handoffCard],
  );

  useEffect(() => {
    syncHasValue(ref.current?.value ?? "", attachmentsRef.current);
  }, [inboxCard, noteCard, handoffCard, syncHasValue]);

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
    const apply = (next: ProjectFile[]) => {
      if (!cancelled) setFiles(next);
    };
    const cached = peekProjectFiles(cwd);
    if (cached) apply(cached);
    void loadProjectFiles(cwd, mentionOpen)
      .then(apply)
      .catch(() => undefined);
    const unsub = subscribeProjectFiles(() => {
      const next = peekProjectFiles(cwd);
      if (next) apply(next);
    });
    return () => {
      cancelled = true;
      unsub();
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
      const planCommand = skill.name === PLAN_COMMAND.name;
      const next = planCommand
        ? `${el.value.slice(0, token.start)}${el.value
            .slice(token.end)
            .replace(/^\s/, "")}`
        : replaceSlashToken(el.value, token, skill.invocation);
      el.value = next;
      resizeTextarea(el);
      let cursor = planCommand
        ? token.start
        : token.start + skill.invocation.length + 1;
      if (next[cursor] === " ") cursor += 1;
      el.setSelectionRange(cursor, cursor);
      setDraft(next);
      syncHasValue(next, attachmentsRef.current);
      setSlash(null);
      setCreatingSkill(false);
      if (planCommand) setPlanSelected(true);
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
        "[data-model-picker], [data-access-picker], [data-model-settings], [data-file-picker], [data-branch-picker], [data-skill-picker], [data-mention-picker], [data-composer-plus]",
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
    if (isCompactCommand(value)) {
      if (!onCompactContext?.()) return;
      if (!ref.current) return;
      ref.current.value = "";
      ref.current.style.height = "auto";
      setDraft("");
      setPlusOpen(false);
      setSlash(null);
      setMention(null);
      setCreatingSkill(false);
      setCreateError(null);
      syncHasValue("", attachments);
      return;
    }

    const command = consumePlanCommand(value);
    const text = composeInboxMessage(inboxCard, command.text);
    const files = attachments;
    if (!text && files.length === 0 && !noteCard && !handoffCard) return;
    onSubmit(text, files, {
      intent: planSelected || command.planning ? "plan" : "default",
    });
    if (!ref.current) return;
    ref.current.value = "";
    ref.current.style.height = "auto";
    setDraft("");
    setAttachments([]);
    setPlanSelected(false);
    setPlusOpen(false);
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

    if (
      e.key === "Enter" &&
      !e.shiftKey &&
      isCompactCommand(e.currentTarget.value)
    ) {
      e.preventDefault();
      submit(e.currentTarget.value);
      return;
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
      {question && onQuestionReply ? (
        <QuestionForm prompt={question} onReply={onQuestionReply} />
      ) : null}
      {children}
      <MessageQueue
        messages={queuedMessages}
        status={queueStatus}
        onDelete={onDeleteQueuedMessage}
        onEdit={onEditQueuedMessage}
        onEditingChange={onQueuedMessageEditingChange}
        onSteer={onSteerQueuedMessage}
        onResume={onResumeQueue}
      />
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
              loading={looksLikeProject(cwd) && peekProjectFiles(cwd) == null}
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
              <ContextMeter
                usage={context}
                onCompact={compactSupported ? onCompactContext : undefined}
                compactDisabled={busy}
              />
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

          {handoffCard ? (
            <HandoffMiniCard
              card={handoffCard}
              onDismiss={onHandoffCardDismiss}
            />
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
                    : handoffCard
                      ? "Add context, or send to continue…"
                      : shell
                        ? "How can I help you today?"
                        : "Ask, build, / for commands, @ for references... "
              }
              className={`composer-field scrollbar-none relative max-h-40 w-full resize-none overflow-x-hidden whitespace-pre-wrap break-words bg-transparent px-3 text-sm leading-5.5 outline-none placeholder:overflow-hidden placeholder:text-ellipsis placeholder:whitespace-nowrap font-sans ${
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
            <div ref={plusRef} className="relative shrink-0">
              <ToolButton
                label="Add files or choose a mode"
                active={plusOpen}
                onClick={() => setPlusOpen((open) => !open)}
              >
                <Plus className="size-3.5" strokeWidth={1.5} />
              </ToolButton>
              {plusOpen ? (
                <Popover
                  anchor={plusRef}
                  side="top"
                  align="start"
                  width={250}
                  onDismiss={() => setPlusOpen(false)}
                  data-composer-plus
                  className="p-1.5"
                >
                  <p className="px-2 pb-1 pt-0.5 text-[10px] font-medium uppercase tracking-wide text-content/40">
                    Add to message
                  </p>
                  <button
                    type="button"
                    disabled={!attachmentsSupported}
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => {
                      setPlusOpen(false);
                      attachFromPicker();
                    }}
                    className="flex w-full items-start gap-2.5 rounded-lg px-2 py-2 text-left text-content hover:bg-content/10 disabled:cursor-not-allowed disabled:opacity-40"
                  >
                    <FilePlus className="mt-0.5 size-4 shrink-0" />
                    <span className="min-w-0">
                      <span className="block text-[13px]">Upload file</span>
                      <span className="block text-[11px] leading-4 text-content/45">
                        {attachmentsSupported
                          ? "Attach files or images to this message"
                          : `${HARNESS_TITLE[harness]} does not support attachments`}
                      </span>
                    </span>
                  </button>
                  <button
                    type="button"
                    aria-pressed={planSelected}
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => {
                      setPlanSelected((selected) => !selected);
                      setPlusOpen(false);
                      ref.current?.focus();
                    }}
                    className="flex w-full items-start gap-2.5 rounded-lg px-2 py-2 text-left text-content hover:bg-content/10"
                  >
                    <AiIdea className="mt-0.5 size-4 shrink-0 text-yellow-300/80" />
                    <span className="min-w-0 flex-1">
                      <span className="block text-[13px]">Plan mode</span>
                      <span className="block text-[11px] leading-4 text-content/45">
                        Create a plan to review before building
                      </span>
                    </span>
                    {planSelected ? (
                      <Check className="mt-0.5 size-3.5 shrink-0 text-accent" />
                    ) : null}
                  </button>
                </Popover>
              ) : null}
            </div>
            {planSelected ? (
              <button
                type="button"
                title="Turn off Plan mode"
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => {
                  setPlanSelected(false);
                  ref.current?.focus();
                }}
                className="flex h-6.5 shrink-0 items-center gap-1 rounded-md bg-yellow-300/12 px-1.5 text-[11px] text-yellow-200/90 hover:bg-yellow-300/18"
              >
                <AiIdea className="size-3.5" />
                Plan
                <X className="size-3" />
              </button>
            ) : null}
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

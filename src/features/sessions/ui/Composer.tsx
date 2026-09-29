import {
  ArrowUp,
  AiIdea,
  Check,
  CircleDashed,
  CornerDownRight,
  CursorMagicSelection,
  FilePlus,
  ListEnd,
  Pause,
  Pencil,
  Play,
  Plus,
  Share,
  Square,
  StickyNote,
  Trash2,
  X,
} from "../../../shared/ui/icons";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type ClipboardEvent,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  attachmentsFromFiles,
  attachmentsFromPaths,
  filesFromClipboard,
  mergeAttachments,
  pickAttachments,
  revokeAttachment,
} from "../model/attachments";
import { resizeComposer } from "../model/composerResize";
import {
  isFileReferenceText,
  messageFilesFromClipboard,
  nativeClipboardAttachments,
} from "../../../platform/tauri/clipboard";
import {
  EXPLORER_FILE_POINTER_DRAG_EVENT,
  type ExplorerFilePointerDragDetail,
} from "../../../shared/lib/drag";
import type { ContextUsage } from "../model/contextUsage";
import {
  loadProjectFiles,
  peekProjectFiles,
  recentOpenedFiles,
  subscribeProjectFiles,
} from "../../files/model/fileIndex";
import {
  buildMentionIndex,
  fileMentionParts,
  mentionLabel,
  mentionTokenAt,
  rankMentionFiles,
  replaceMentionToken,
  type MentionIndex,
  type MentionToken,
} from "../../files/model/fileMentions";
import type { ProjectFile } from "../../../platform/tauri/fs";
import {
  composeInboxMessage,
  type InboxComposerCard,
} from "../../inbox/model/githubTasks";
import type { HandoffComposerCard } from "../model/handoff";
import {
  looksLikeProject,
  type RecentProject,
} from "../../projects/model/recents";
import type {
  Attachment,
  HarnessId,
  MessageQueueStatus,
  QueuedMessage,
  UsageLimit,
  RuntimeMode,
  WorkspaceMode,
  ComposerTurnOptions,
} from "../model/session";
import { HARNESS_TITLE, harnessSupportsAttachments } from "../model/session";
import type {
  UserQuestionPrompt,
  UserQuestionReply,
} from "../model/userQuestion";
import { isImeComposition } from "../../../shared/lib/keyboard";
import {
  captureDraft,
  dropPastedText,
  insertRestoredText,
} from "../../../shared/lib/draftRestore";
import {
  createBlankSkill,
  rankSkills,
  hasNativeCommands,
  isNativeCommandPrompt,
  replaceSlashToken,
  skillTextParts,
  slashTokenAt,
  type Skill,
  type SlashToken,
} from "../../skills/model/skills";
import { AccessPicker } from "./AccessPicker";
import { ComposerRunner } from "./ComposerRunner";
import { ContextMeter } from "./ContextMeter";
import { AttachmentChip } from "./AttachmentChip";
import { BranchPicker } from "../../source-control/ui/BranchPicker";
import { WorktreePicker } from "../../source-control/ui/WorktreePicker";
import {
  isWorkspaceModeShortcut,
  WorkspaceIdentity,
  WorkspacePicker,
} from "../../workspace/ui/WorkspacePicker";
import type { Worktree } from "../../source-control/model/worktrees";
import { CwdPicker } from "../../projects/ui/CwdPicker";
import { FileMentionPicker } from "./FileMentionPicker";
import { FileTypeIcon } from "../../files/ui/FileTypeIcon";
import { InboxMiniCard } from "../../inbox/ui/InboxMiniCard";
import { NoteMiniCard } from "../../notes/ui";
import { HandoffMiniCard } from "./HandoffMiniCard";
import { ModelControlPills, ModelPicker } from "./ModelPicker";
import { QuestionForm } from "./QuestionForm";
import { SkillPicker } from "../../skills/ui/SkillPicker";
import { pathKey, projectKey } from "../../../shared/lib/paths";
import {
  consumeComposerInsert,
  type ComposerInsertRequest,
} from "../model/quoteDraft";
import {
  chatContextKey,
  chatContextSummary,
  composeChatContext,
  splitChatContext,
  type ChatContextItem,
} from "../model/chatContext";
import { ChatContextChip } from "./ChatContextChip";
import type { OpenFileFn } from "../../search/model/search";
import { useTabGroupLogos } from "../../projects/hooks/useTabGroupLogos";
import { useProjectBranchesState } from "../../source-control/hooks/useProjectBranches";
import {
  COMPOSER_RUNNER_CHANGE_EVENT,
  loadComposerRunner,
  loadModelControls,
  loadNotesEnabled,
  subscribeModelControls,
  subscribeNotesEnabled,
} from "../../settings/model/settings";
import {
  isNoteMentionPath,
  loadNotes,
  peekNotes,
  rankNoteFiles,
  notesAsProjectFiles,
  type Note,
  type NoteComposerCard,
} from "../../notes";
import { resolveTabGroupLogo } from "../../workspace/model/tabGroups";
import { useComposerSkills } from "./useComposerSkills";
import { Popover } from "../../../shared/ui/Popover";
import { UsageLimitNotice } from "./UsageLimitNotice";
import { consumePlanCommand, PLAN_COMMAND } from "../model/plan";
import {
  consumeOperatorCommand,
  OPERATOR_COMMAND,
} from "../model/operatorCommand";
import {
  BTW_COMMAND,
  consumeBtwCommand,
  consumeBtwPrefix,
  supportsBtwHarness,
} from "../model/btw";
import { COMPACT_COMMAND, isCompactCommand } from "../model/compact";
import {
  consumeSessionFolderCommand,
  isSessionFolderCommand,
  runsSessionFolderCommandOnSpace,
  SESSION_FOLDER_COMMAND,
} from "../model/sessionFolderCommand";
import {
  loadSessionFolders,
  type SessionFolderTarget,
  type SessionFolder,
} from "../model/sessionFolders";
import { SessionFolderPicker } from "./SessionFolderPicker";
import type { LastTurnRecall } from "../model/editLastTurn";

type Props = {
  enabled?: boolean;
  focused: boolean;
  /** Bump to force a refocus even when `focused` was already true (e.g. window regains OS focus). */
  focusToken?: number;
  shell?: boolean;
  compact?: boolean;
  placeholder?: string;
  inputAriaLabel?: string;
  disabled?: boolean;
  allowedModelHarnesses?: readonly HarnessId[];
  harness: HarnessId;
  model: string;
  modelSettings?: Record<string, string>;
  runtimeMode: RuntimeMode;
  cwd?: string;
  executionCwd: string;
  sessionId?: string;
  branch?: string;
  recents?: RecentProject[];
  hideProjectPicker?: boolean;
  hideBranchPicker?: boolean;
  hideTopBar?: boolean;
  /** Keeps local file mentions, skills, and app modes off for host sessions. */
  remoteSession?: boolean;
  remoteFeatures?: { attachments: boolean; plan: boolean; draft: boolean };
  context?: ContextUsage;
  compactSupported?: boolean;
  insertRequest?: ComposerInsertRequest;
  /** Draft text. A trailing context block becomes chips. */
  initialDraft?: string;
  draftResetToken?: number;
  inboxCard?: InboxComposerCard;
  noteCard?: NoteComposerCard;
  handoffCard?: HandoffComposerCard;
  question?: UserQuestionPrompt;
  busy?: boolean;
  /** Allow typed text to replace Stop with Send while a turn is running. */
  allowBusySubmit?: boolean;
  editLastTurnSupported?: boolean;
  lastTurnRecall?: LastTurnRecall | null;
  queuedMessages?: QueuedMessage[];
  queueStatus?: MessageQueueStatus;
  usageLimit?: UsageLimit;
  hotkeys?: boolean;
  onFocus: () => void;
  onCwdChange: (cwd: string) => void;
  onBranchChange?: () => void;
  onWorktreeChange?: (tree: Worktree) => Promise<void>;
  draftWorkspace?: boolean;
  workspaceMode?: WorkspaceMode;
  worktreeBase?: string;
  onWorkspaceModeChange?: (mode: WorkspaceMode, base?: string) => void;
  onWorktreeBaseChange?: (base: string) => void;
  worktreeRemoved?: boolean;
  onManageWorktrees?: () => void;
  onNewTerminal?: () => void;
  onModelChange: (harness: HarnessId, model: string) => void;
  onModelSettingsChange?: (settings: Record<string, string>) => void;
  onRuntimeModeChange: (mode: RuntimeMode) => void;
  onInsertRequestConsumed?: (id: number) => void;
  onInboxCardDismiss?: () => void;
  onNoteCardDismiss?: () => void;
  onHandoffCardDismiss?: () => void;
  onQuestionReply?: (requestId: number, reply: UserQuestionReply) => void;
  onQuestionInteraction?: (requestId: number) => void;
  onSubmit: (
    text: string,
    attachments: Attachment[],
    options?: ComposerTurnOptions,
  ) => boolean | void;
  /** `draft` opens the side question with the text unsent, for a typed `/btw `. */
  onBtwCommand?: (
    text: string,
    options?: { draft?: boolean },
  ) => boolean | void;
  canSaveDraft?: boolean;
  onSaveDraft?: (text: string, attachments: Attachment[]) => boolean | void;
  onStop?: () => void;
  onCompactContext?: () => boolean;
  onPlaceInFolder?: (target: SessionFolderTarget) => void;
  onDeleteQueuedMessage?: (messageId: string) => void;
  onEditQueuedMessage?: (messageId: string, text: string) => void;
  onQueuedMessageEditingChange?: (messageId?: string) => void;
  onSteerQueuedMessage?: (messageId: string) => void;
  onResumeQueue?: () => void;
  onUsageLimitResume?: () => void;
  onUsageLimitResumeAtReset?: (enabled: boolean) => void;
  onUsageLimitDismiss?: () => void;
  onOpenFile?: OpenFileFn;
  /** Receives the draft with its context chips appended as a context block. */
  onDraftChange?: (text: string) => void;
  onRecallLastTurnReady?: (recall: () => void) => void;
  onEditingLastTurnChange?: (editing: boolean) => void;
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
          ? "bg-selection-emphasis text-content"
          : "bg-selection text-content/50 hover:bg-selection-hover hover:text-content"
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
    setEditDraft(splitChatContext(message.text).text);
    onEditingChange?.(message.id);
  };
  const cancelEdit = () => {
    setEditingId(undefined);
    setEditDraft("");
    onEditingChange?.();
  };
  // The editor only holds the typed text; the message keeps its context chips.
  const saveEdit = (message: QueuedMessage) => {
    const { items } = splitChatContext(message.text);
    if (!editDraft.trim() && message.attachments.length === 0 && !items.length)
      return;
    onEdit?.(message.id, composeChatContext(editDraft, items));
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
          <div className="flex h-7 items-center gap-2 border-b border-stroke text-[12px]">
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
          const { text, items } = splitChatContext(message.text);
          const label =
            text.trim() ||
            chatContextSummary(items) ||
            `${message.attachments.length} attachment${message.attachments.length === 1 ? "" : "s"}`;
          return (
            <div
              key={message.id}
              className={`flex min-h-7 items-center gap-2 text-[12px] ${
                index > 0 ? "border-t border-stroke" : ""
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
                      if (isImeComposition(event.nativeEvent)) return;
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
                      !editDraft.trim() &&
                      message.attachments.length === 0 &&
                      !items.length
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
  focusToken,
  hotkeys = false,
  shell = false,
  compact = false,
  placeholder,
  inputAriaLabel,
  disabled = false,
  harness,
  model,
  allowedModelHarnesses,
  modelSettings = {},
  runtimeMode,
  cwd = "~",
  executionCwd,
  sessionId,
  branch,
  recents = [],
  hideProjectPicker = false,
  hideBranchPicker = false,
  hideTopBar = false,
  remoteSession = false,
  remoteFeatures,
  context,
  compactSupported = false,
  insertRequest,
  initialDraft,
  draftResetToken,
  inboxCard,
  noteCard,
  handoffCard,
  question,
  busy = false,
  allowBusySubmit = true,
  editLastTurnSupported = false,
  lastTurnRecall = null,
  queuedMessages = [],
  queueStatus,
  usageLimit,
  onFocus,
  onCwdChange,
  onBranchChange,
  onWorktreeChange,
  draftWorkspace = false,
  workspaceMode,
  worktreeBase,
  onWorkspaceModeChange,
  onWorktreeBaseChange,
  worktreeRemoved = false,
  onManageWorktrees,
  onNewTerminal,
  onModelChange,
  onModelSettingsChange,
  onRuntimeModeChange,
  onInsertRequestConsumed,
  onInboxCardDismiss,
  onBtwCommand,
  onNoteCardDismiss,
  onHandoffCardDismiss,
  onQuestionReply,
  onQuestionInteraction,
  onSubmit,
  canSaveDraft = false,
  onSaveDraft,
  onStop,
  onCompactContext,
  onPlaceInFolder,
  onDeleteQueuedMessage,
  onEditQueuedMessage,
  onQueuedMessageEditingChange,
  onSteerQueuedMessage,
  onResumeQueue,
  onUsageLimitResume,
  onUsageLimitResumeAtReset,
  onUsageLimitDismiss,
  onOpenFile,
  onDraftChange,
  onRecallLastTurnReady,
  onEditingLastTurnChange,
  children,
}: Props) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const boxRef = useRef<HTMLDivElement>(null);
  const plusRef = useRef<HTMLDivElement>(null);
  const highlightRef = useRef<HTMLDivElement>(null);
  const attachmentsRef = useRef<Attachment[]>([]);
  const borrowedAttachmentIdsRef = useRef(new Set<string>());
  const attachmentLifecycleRef = useRef(0);
  const consumedInsertId = useRef<number | null>(null);
  const draftRevisionRef = useRef(0);
  const draftResetTokenRef = useRef(draftResetToken);
  /** Bumped when the draft is cleared, so a late paste cannot land on the next one. */
  const pasteGenerationRef = useRef(0);
  /** Native and file pastes still reading when Send is pressed. */
  const pasteFlightRef = useRef<Promise<void> | null>(null);
  const submitLockRef = useRef(false);
  const positionedInitialDraft = useRef(false);
  const slashRef = useRef<SlashToken | null>(null);
  const mentionRef = useRef<MentionToken | null>(null);
  const initialMessage = useMemo(
    () => (initialDraft == null ? undefined : splitChatContext(initialDraft)),
    [initialDraft],
  );
  const initialText = initialMessage?.text;
  const [draft, setDraft] = useState(initialText ?? "");
  const [contextItems, setContextItems] = useState<ChatContextItem[]>(
    () => initialMessage?.items ?? [],
  );
  const contextItemsRef = useRef(contextItems);
  contextItemsRef.current = contextItems;
  const { branches: draftBranches } = useProjectBranchesState(
    executionCwd,
    draftWorkspace && enabled && !busy,
  );
  const resolvedWorktreeBase =
    worktreeBase && worktreeBase !== "HEAD"
      ? worktreeBase
      : branch || draftBranches?.current || worktreeBase || undefined;
  useEffect(() => {
    if (
      draftWorkspace &&
      workspaceMode === "worktree" &&
      worktreeBase === "HEAD" &&
      draftBranches?.current
    ) {
      onWorktreeBaseChange?.(draftBranches.current);
    }
  }, [
    draftBranches?.current,
    draftWorkspace,
    onWorktreeBaseChange,
    workspaceMode,
    worktreeBase,
  ]);
  const [hasValue, setHasValue] = useState(
    () =>
      (initialDraft ?? "").trim().length > 0 ||
      !!inboxCard ||
      !!noteCard ||
      !!handoffCard,
  );
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [pasteError, setPasteError] = useState<string | null>(null);
  const [fileDrag, setFileDrag] = useState(false);
  const [plusOpen, setPlusOpen] = useState(false);
  const [planSelected, setPlanSelected] = useState(false);
  const [operatorSelected, setOperatorSelected] = useState(false);
  const [orchestrationSelected, setOrchestrationSelected] = useState(false);
  const [draftSelected, setDraftSelected] = useState(false);
  const [slash, setSlash] = useState<SlashToken | null>(null);
  const [skillActive, setSkillActive] = useState(0);
  const [creatingSkill, setCreatingSkill] = useState(false);
  const [sessionFolderOpen, setSessionFolderOpen] = useState(false);
  const [sessionFolders, setSessionFolders] = useState<SessionFolder[]>([]);
  const [sessionFolderSelected, setSessionFolderSelected] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [createBusy, setCreateBusy] = useState(false);
  const remote = remoteSession;
  // Local indexes (files, skills) must never read a remote session's path.
  const localCwd = remote ? "" : executionCwd;
  const [files, setFiles] = useState<ProjectFile[]>(
    () => peekProjectFiles(localCwd) ?? [],
  );
  const notesEnabled = useSyncExternalStore(
    subscribeNotesEnabled,
    loadNotesEnabled,
    () => true,
  );
  const modelControls = useSyncExternalStore(
    subscribeModelControls,
    loadModelControls,
    () => "menu" as const,
  );
  const controlsBeside = modelControls === "beside";
  const [notes, setNotes] = useState<Note[]>(() => peekNotes() ?? []);
  const [mention, setMention] = useState<MentionToken | null>(null);
  const [mentionActive, setMentionActive] = useState(0);
  const [resendEdited, setResendEdited] = useState(false);
  const [runnerEnabled, setRunnerEnabled] = useState(loadComposerRunner);
  const [runnerLive, setRunnerLive] = useState(
    () => busy && loadComposerRunner(),
  );
  const groupLogos = useTabGroupLogos();
  const projectLogoPath = resolveTabGroupLogo(projectKey(cwd), groupLogos);

  slashRef.current = slash;
  mentionRef.current = mention;

  attachmentsRef.current = attachments;

  const mentionOpen =
    !remote && mention !== null && (looksLikeProject(cwd) || notesEnabled);
  const navigationEmpty =
    draft.length === 0 &&
    attachments.length === 0 &&
    contextItems.length === 0 &&
    !inboxCard &&
    !noteCard &&
    !handoffCard;
  const skillPickerOpen = creatingSkill || slash !== null;
  const pickerOpen = skillPickerOpen || sessionFolderOpen;
  const skillCatalog = useComposerSkills({
    harness,
    executionCwd: localCwd,
    sessionId,
    pickerOpen: pickerOpen && !remote,
  });
  const skills = skillCatalog.skills;
  const slashItems = useMemo(
    () =>
      remote
        ? [...(remoteFeatures?.plan ? [PLAN_COMMAND] : []), COMPACT_COMMAND]
        : [
            SESSION_FOLDER_COMMAND,
            OPERATOR_COMMAND,
            PLAN_COMMAND,
            COMPACT_COMMAND,
            ...(supportsBtwHarness(harness) ? [BTW_COMMAND] : []),
            ...skills.filter(
              (skill) =>
                ![OPERATOR_COMMAND.name, "mono", "monocode"].includes(
                  skill.name,
                ) &&
                (skill.kind === "native" ||
                  (skill.name !== PLAN_COMMAND.name &&
                    skill.name !== COMPACT_COMMAND.name &&
                    skill.name !== SESSION_FOLDER_COMMAND.name &&
                    skill.name !== BTW_COMMAND.name)),
            ),
          ],
    [harness, skills, remote, remoteFeatures?.plan],
  );
  const skillLimit = hasNativeCommands(harness)
    ? Number.POSITIVE_INFINITY
    : undefined;
  const rankedSkills = rankSkills(slashItems, slash?.query ?? "", skillLimit);
  const attachmentsSupported =
    (!remote || !!remoteFeatures?.attachments) &&
    harnessSupportsAttachments(harness);
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
    const fileHits = looksLikeProject(executionCwd)
      ? rankMentionFiles(
          files,
          mention?.query ?? "",
          recentOpenedFiles(executionCwd),
        )
      : [];
    const noteHits = notesEnabled
      ? rankNoteFiles(notes, mention?.query ?? "")
      : [];
    const seen = new Set(noteHits.map((file) => file.path));
    return [...noteHits, ...fileHits.filter((file) => !seen.has(file.path))];
  }, [executionCwd, files, mention?.query, mentionOpen, notes, notesEnabled]);

  const hasContext = contextItems.length > 0;
  const syncHasValue = useCallback(
    (text: string, files: Attachment[]) => {
      setHasValue(
        text.trim().length > 0 ||
          files.length > 0 ||
          hasContext ||
          !!inboxCard ||
          !!noteCard ||
          !!handoffCard,
      );
    },
    [hasContext, inboxCard, noteCard, handoffCard],
  );

  useEffect(() => {
    syncHasValue(ref.current?.value ?? "", attachmentsRef.current);
  }, [inboxCard, noteCard, handoffCard, syncHasValue]);

  const addAttachments = useCallback(
    (incoming: Attachment[]) => {
      if (!harnessSupportsAttachments(harness) || incoming.length === 0) return;
      const next = mergeAttachments(attachmentsRef.current, incoming);
      attachmentsRef.current = next;
      setAttachments(next);
      setPasteError(null);
      draftRevisionRef.current += 1;
      syncHasValue(ref.current?.value ?? "", next);
      ref.current?.focus();
    },
    [harness, syncHasValue],
  );

  const removeAttachment = useCallback(
    (id: string) => {
      const previous = attachmentsRef.current;
      const removed = previous.find((file) => file.id === id);
      if (removed && !borrowedAttachmentIdsRef.current.delete(removed.id)) {
        revokeAttachment(removed);
      }
      const next = previous.filter((file) => file.id !== id);
      attachmentsRef.current = next;
      draftRevisionRef.current += 1;
      setAttachments(next);
      setPasteError(null);
      syncHasValue(ref.current?.value ?? "", next);
      ref.current?.focus();
    },
    [syncHasValue],
  );
  useEffect(() => {
    const lifecycle = ++attachmentLifecycleRef.current;
    return () => {
      queueMicrotask(() => {
        if (attachmentLifecycleRef.current !== lifecycle) return;
        for (const file of attachmentsRef.current) {
          if (!borrowedAttachmentIdsRef.current.delete(file.id)) {
            revokeAttachment(file);
          }
        }
        attachmentsRef.current = [];
        borrowedAttachmentIdsRef.current.clear();
      });
    };
  }, []);

  useEffect(() => {
    if (harnessSupportsAttachments(harness)) return;
    const previous = attachmentsRef.current;
    if (previous.length === 0) return;
    for (const file of previous) {
      if (!borrowedAttachmentIdsRef.current.delete(file.id)) {
        revokeAttachment(file);
      }
    }
    attachmentsRef.current = [];
    setAttachments([]);
    syncHasValue(ref.current?.value ?? "", []);
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
    setSessionFolderOpen(false);
    setSessionFolderSelected(false);
  }, [cwd]);

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
    const cached = peekProjectFiles(localCwd);
    apply(cached ?? []);
    if (!localCwd) return;
    void loadProjectFiles(localCwd, mentionOpen)
      .then(apply)
      .catch(() => undefined);
    const unsub = subscribeProjectFiles(() => {
      const next = peekProjectFiles(localCwd);
      if (next) apply(next);
    });
    return () => {
      cancelled = true;
      unsub();
    };
  }, [localCwd, mentionOpen]);

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

  useEffect(() => {
    const el = ref.current;
    if (!el || !initialText) return;
    if (el.value !== initialText) el.value = initialText;
    if (!positionedInitialDraft.current) {
      const end = el.value.length;
      el.setSelectionRange(end, end);
      positionedInitialDraft.current = true;
    }
    resizeComposer(el);
  }, [initialText]);

  // Drafts changed while hidden could not be measured. Inbox panes are portaled
  // into place by a parent effect that runs after this one, so the first pass
  // can still find no layout box; retry once the move has landed.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || !enabled) return;
    resizeComposer(el);
    if (el.scrollHeight !== 0) return;
    const frame = requestAnimationFrame(() => {
      if (ref.current === el) resizeComposer(el);
    });
    return () => cancelAnimationFrame(frame);
  }, [enabled]);

  useEffect(() => {
    onDraftChange?.(composeChatContext(draft, contextItems));
  }, [contextItems, draft, onDraftChange]);
  useEffect(() => {
    if (
      draftResetToken == null ||
      draftResetTokenRef.current === draftResetToken
    ) {
      return;
    }
    draftResetTokenRef.current = draftResetToken;
    pasteGenerationRef.current += 1;
    draftRevisionRef.current += 1;
    if (ref.current) {
      ref.current.value = "";
      ref.current.style.height = "auto";
    }
    setDraft("");
    contextItemsRef.current = [];
    setContextItems([]);
    onDraftChange?.("");
    setDraftSelected(false);
    setPlanSelected(false);
    setOrchestrationSelected(false);
    setSessionFolderSelected(false);
    setSessionFolderOpen(false);
    setPlusOpen(false);
    setSlash(null);
    setMention(null);
    setCreatingSkill(false);
    setCreateError(null);
    syncHasValue("", attachmentsRef.current);
  }, [draftResetToken, onDraftChange, syncHasValue]);

  const syncHighlightScroll = useCallback((el: HTMLTextAreaElement) => {
    const highlight = highlightRef.current;
    if (!highlight) return;
    highlight.scrollTop = el.scrollTop;
    highlight.scrollLeft = el.scrollLeft;
  }, []);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;

    // The textarea can scroll itself to keep the caret visible before React
    // commits the updated highlight text. Sync again after that commit, when
    // the overlay has enough scrollable content to accept the same offset.
    syncHighlightScroll(el);
    const frame = requestAnimationFrame(() => {
      if (ref.current === el) syncHighlightScroll(el);
    });
    return () => cancelAnimationFrame(frame);
  }, [draft, syncHighlightScroll]);

  const syncTokensFromTextarea = (el: HTMLTextAreaElement) => {
    if (creatingSkill) return;
    const cursor = el.selectionStart ?? 0;
    const token = slashTokenAt(el.value, cursor, hasNativeCommands(harness));
    setSlash(token);
    setMention(token ? null : mentionTokenAt(el.value, cursor));
  };

  const openSessionFolderPicker = useCallback(() => {
    setSessionFolders(loadSessionFolders(cwd));
    setSessionFolderOpen(true);
  }, [cwd]);

  useEffect(() => {
    const el = ref.current;
    if (!el || !insertRequest) return;

    const fresh = insertRequest.id !== consumedInsertId.current;
    const result = consumeComposerInsert(
      el.value,
      contextItemsRef.current,
      consumedInsertId.current,
      insertRequest,
    );
    consumedInsertId.current = result.consumedId;
    if (result.changed) {
      el.value = result.draft;
      resizeComposer(el);
      setDraft(result.draft);
      contextItemsRef.current = result.context;
      setContextItems(result.context);
      syncHasValue(result.draft, attachmentsRef.current);
      setSlash(null);
      setMention(null);
      setCreatingSkill(false);
      setCreateError(null);
      el.setSelectionRange(result.draft.length, result.draft.length);
    }
    // Adding the same chip twice changes nothing, but the user still expects
    // to land in the composer.
    if (fresh) el.focus();
    onInsertRequestConsumed?.(insertRequest.id);
  }, [onInsertRequestConsumed, insertRequest, syncHasValue]);

  const removeContextItem = useCallback((item: ChatContextItem) => {
    const key = chatContextKey(item);
    const next = contextItemsRef.current.filter(
      (entry) => chatContextKey(entry) !== key,
    );
    contextItemsRef.current = next;
    draftRevisionRef.current += 1;
    setContextItems(next);
    ref.current?.focus();
  }, []);

  // `/btw ` opens the side conversation as soon as it is typed, carrying any
  // text after it over as the unsent side question.
  const enterBtwFromPrefix = useCallback(
    (el: HTMLTextAreaElement) => {
      if (!onBtwCommand || inboxCard || noteCard || handoffCard) return false;
      if (attachmentsRef.current.length > 0) return false;
      const rest = consumeBtwPrefix(el.value);
      if (rest == null || onBtwCommand(rest, { draft: true }) === false) {
        return false;
      }
      el.value = "";
      resizeComposer(el);
      draftRevisionRef.current += 1;
      setDraft("");
      onDraftChange?.("");
      syncHasValue("", attachmentsRef.current);
      setSlash(null);
      setMention(null);
      return true;
    },
    [
      handoffCard,
      inboxCard,
      noteCard,
      onBtwCommand,
      onDraftChange,
      syncHasValue,
    ],
  );

  const pickSkill = useCallback(
    (skill: Skill) => {
      const el = ref.current;
      const token = slashRef.current;
      if (!el || !token) {
        setSlash(null);
        setCreatingSkill(false);
        return;
      }
      const planCommand =
        skill.kind === "builtin" && skill.name === PLAN_COMMAND.name;
      const sessionFolderCommand =
        skill.kind === "builtin" &&
        skill.name === SESSION_FOLDER_COMMAND.name &&
        !!onPlaceInFolder;
      if (sessionFolderCommand) {
        const next = replaceSlashToken(
          el.value,
          token,
          SESSION_FOLDER_COMMAND.invocation,
        );
        el.value = next;
        resizeComposer(el);
        let cursor = token.start + SESSION_FOLDER_COMMAND.invocation.length + 1;
        if (next[cursor] === " ") cursor += 1;
        el.setSelectionRange(cursor, cursor);
        setDraft(next);
        syncHasValue(next, attachmentsRef.current);
        setSlash(null);
        setCreatingSkill(false);
        openSessionFolderPicker();
        return;
      }
      const next = planCommand
        ? `${el.value.slice(0, token.start)}${el.value
            .slice(token.end)
            .replace(/^\s/, "")}`
        : replaceSlashToken(el.value, token, skill.invocation);
      el.value = next;
      resizeComposer(el);
      let cursor = planCommand
        ? token.start
        : token.start + skill.invocation.length + 1;
      if (next[cursor] === " ") cursor += 1;
      el.setSelectionRange(cursor, cursor);
      setDraft(next);
      syncHasValue(next, attachmentsRef.current);
      setSlash(null);
      setCreatingSkill(false);
      if (skill.kind === "builtin" && skill.name === BTW_COMMAND.name) {
        enterBtwFromPrefix(el);
      }
      if (planCommand) {
        setPlanSelected(true);
        setOperatorSelected(false);
        setOrchestrationSelected(false);
      }
      el.focus();
    },
    [
      enterBtwFromPrefix,
      onPlaceInFolder,
      openSessionFolderPicker,
      syncHasValue,
    ],
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
      resizeComposer(el);
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
    if (!focused || disabled) return;

    const composer = ref.current?.closest("[data-composer]");
    const activeComposer = document.activeElement?.closest("[data-composer]");
    const activeComposerHidden = activeComposer?.closest(
      '[aria-hidden="true"], [inert]',
    );
    // Inactive workspace tabs stay mounted, so focus can still be sitting in
    // their composer when a new session becomes active. Only preserve focus
    // for another composer that is still visible (for example, a split pane).
    if (activeComposer && activeComposer !== composer && !activeComposerHidden)
      return;

    if (
      composer?.querySelector(
        "[data-skill-picker], [data-session-folder-picker], [data-mention-picker], [data-composer-plus], [data-question-form]",
      )
    )
      return;
    // Model/access/branch/settings/file pickers render through a portal into
    // document.body (see Popover.tsx), so they never appear under this
    // composer's own DOM subtree — check the whole document for those.
    if (
      document.querySelector(
        "[data-model-picker], [data-access-picker], [data-model-settings], [data-file-picker], [data-branch-picker]",
      )
    )
      return;
    ref.current?.focus();
  }, [disabled, focused, question, busy, focusToken]);

  useEffect(() => {
    if (!enabled || disabled) {
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

    const onExplorerFilePointerDrag = (event: Event) => {
      if (remote) return;
      const detail = (event as CustomEvent<ExplorerFilePointerDragDetail>)
        .detail;
      if (!detail || detail.type === "end") {
        setFileDrag(false);
        return;
      }
      const over = overTarget(detail.x, detail.y);
      if (detail.type === "move") {
        setFileDrag(over && attachmentsSupported);
        return;
      }
      setFileDrag(false);
      if (!over || !attachmentsSupported) return;
      void attachmentsFromPaths([detail.path]).then(addAttachments);
    };

    const root = dropRoot();
    root?.addEventListener("dragover", onDragOver);
    root?.addEventListener("dragleave", onDragLeave);
    root?.addEventListener("drop", onDrop);
    window.addEventListener(
      EXPLORER_FILE_POINTER_DRAG_EVENT,
      onExplorerFilePointerDrag,
    );

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
      window.removeEventListener(
        EXPLORER_FILE_POINTER_DRAG_EVENT,
        onExplorerFilePointerDrag,
      );
      unlisten?.();
    };
  }, [addAttachments, attachmentsSupported, enabled, remote]);
  const restoreDraft = useCallback(
    (
      text: string,
      nextAttachments: Attachment[],
      borrowedIds: ReadonlySet<string> = borrowedAttachmentIdsRef.current,
    ) => {
      const message = splitChatContext(text);
      setDraft(message.text);
      contextItemsRef.current = message.items;
      setContextItems(message.items);
      onDraftChange?.(text);
      if (ref.current) {
        ref.current.value = message.text;
        ref.current.style.height = "auto";
        ref.current.style.height = `${Math.min(ref.current.scrollHeight, 240)}px`;
      }

      const nextIds = new Set(nextAttachments.map((file) => file.id));
      for (const file of attachmentsRef.current) {
        if (
          nextIds.has(file.id) ||
          borrowedAttachmentIdsRef.current.delete(file.id)
        ) {
          continue;
        }
        revokeAttachment(file);
      }
      borrowedAttachmentIdsRef.current = new Set(
        nextAttachments
          .filter((file) => borrowedIds.has(file.id))
          .map((file) => file.id),
      );
      attachmentsRef.current = nextAttachments;
      setAttachments(nextAttachments);
      syncHasValue(message.text, nextAttachments);
      ref.current?.focus();
    },
    [onDraftChange, syncHasValue],
  );

  const exitEditMode = useCallback(() => {
    draftRevisionRef.current += 1;
    pasteGenerationRef.current += 1;
    if (ref.current) {
      ref.current.value = "";
      ref.current.style.height = "auto";
    }
    setDraft("");
    contextItemsRef.current = [];
    setContextItems([]);
    onDraftChange?.("");
    const previous = attachmentsRef.current;
    for (const file of previous) {
      if (!borrowedAttachmentIdsRef.current.delete(file.id)) {
        revokeAttachment(file);
      }
    }
    attachmentsRef.current = [];
    setAttachments([]);
    setResendEdited(false);
    onEditingLastTurnChange?.(false);
    setPlusOpen(false);
    setSlash(null);
    setMention(null);
    syncHasValue("", []);
    ref.current?.focus();
  }, [onDraftChange, onEditingLastTurnChange, syncHasValue]);

  const recallLastTurn = useCallback(() => {
    if (!editLastTurnSupported || !lastTurnRecall) return;
    if (resendEdited) {
      exitEditMode();
      return;
    }
    restoreDraft(
      lastTurnRecall.text,
      lastTurnRecall.attachments,
      new Set(lastTurnRecall.attachments.map((file) => file.id)),
    );
    setResendEdited(true);
    onEditingLastTurnChange?.(true);
  }, [
    editLastTurnSupported,
    exitEditMode,
    lastTurnRecall,
    onEditingLastTurnChange,
    resendEdited,
    restoreDraft,
  ]);

  useEffect(() => {
    draftRevisionRef.current += 1;
    setResendEdited(false);
    onEditingLastTurnChange?.(false);
  }, [sessionId, onEditingLastTurnChange]);

  useEffect(() => {
    if (editLastTurnSupported) return;
    setResendEdited(false);
    onEditingLastTurnChange?.(false);
  }, [editLastTurnSupported, onEditingLastTurnChange]);

  useEffect(() => {
    if (!editLastTurnSupported || !onRecallLastTurnReady) return;
    onRecallLastTurnReady(recallLastTurn);
  }, [editLastTurnSupported, onRecallLastTurnReady, recallLastTurn]);

  const rememberPaste = (work: Promise<void>) => {
    const flight = work.then(
      () => undefined,
      () => undefined,
    );
    const previous = pasteFlightRef.current;
    const joined = previous ? previous.then(() => flight) : flight;
    pasteFlightRef.current = joined;
    void joined.finally(() => {
      if (pasteFlightRef.current === joined) pasteFlightRef.current = null;
    });
  };
  const submit = (value: string) => {
    if (disabled || worktreeRemoved || submitLockRef.current) return;
    submitLockRef.current = true;
    void completeSubmit(value).finally(() => {
      submitLockRef.current = false;
    });
  };
  const completeSubmit = async (submittedValue: string) => {
    let pending = pasteFlightRef.current;
    const generation = pasteGenerationRef.current;
    while (pending) {
      await pending;
      // A reset or an earlier send retired this draft while the read was out.
      if (pasteGenerationRef.current !== generation) return;
      pending = pasteFlightRef.current;
    }
    const value = ref.current?.value ?? submittedValue;
    if (disabled || worktreeRemoved) return;
    const context = contextItemsRef.current;
    if (draftSelected && onSaveDraft) {
      const files = attachmentsRef.current;
      if (!value.trim() && files.length === 0 && context.length === 0) return;
      const accepted = onSaveDraft(composeChatContext(value, context), files);
      if (accepted === false || !ref.current) return;
      pasteGenerationRef.current += 1;
      ref.current.value = "";
      ref.current.style.height = "auto";
      setDraft("");
      contextItemsRef.current = [];
      setContextItems([]);
      onDraftChange?.("");
      setAttachments([]);
      setDraftSelected(false);
      setPlusOpen(false);
      setSlash(null);
      setMention(null);
      setCreatingSkill(false);
      setCreateError(null);
      syncHasValue("", []);
      return;
    }
    const folderCommand = consumeSessionFolderCommand(value);
    const btwCommand = consumeBtwCommand(value);
    if (
      btwCommand.matched &&
      onBtwCommand &&
      attachmentsRef.current.length === 0 &&
      !inboxCard &&
      !noteCard &&
      !handoffCard
    ) {
      const accepted = onBtwCommand(btwCommand.text);
      if (accepted === false) return;
      pasteGenerationRef.current += 1;
      if (ref.current) {
        ref.current.value = "";
        ref.current.style.height = "auto";
      }
      setDraft("");
      onDraftChange?.("");
      setPlusOpen(false);
      setSlash(null);
      setMention(null);
      setCreatingSkill(false);
      setCreateError(null);
      syncHasValue("", []);
      return;
    }
    if (folderCommand.matched && onPlaceInFolder && !sessionFolderSelected) {
      openSessionFolderPicker();
      return;
    }

    if (isCompactCommand(value)) {
      if (!onCompactContext?.()) return;
      if (!ref.current) return;
      pasteGenerationRef.current += 1;
      ref.current.value = "";
      ref.current.style.height = "auto";
      setDraft("");
      onDraftChange?.("");
      setPlusOpen(false);
      setSlash(null);
      setMention(null);
      setCreatingSkill(false);
      setCreateError(null);
      syncHasValue("", attachmentsRef.current);
      return;
    }

    const command = consumePlanCommand(
      folderCommand.matched && sessionFolderSelected
        ? folderCommand.text
        : value,
    );
    const text = composeChatContext(
      isNativeCommandPrompt(command.text, harness)
        ? command.text
        : composeInboxMessage(inboxCard, command.text),
      context,
    );
    const submittedText =
      operatorSelected && !consumeOperatorCommand(text).matched
        ? `/operator ${text}`
        : text;
    const files = attachmentsRef.current;
    if (!text && files.length === 0 && !noteCard && !handoffCard) return;
    // Clear the parent draft before onSubmit. The app can synchronously remount
    // the composer when the first message leaves an empty session (EmptySession →
    // docked layout). If draftRef still holds the sent text, the new instance
    // resurrects it as initialDraft.
    const resendDraftRevision = draftRevisionRef.current;
    const resendBorrowedAttachmentIds = new Set(
      borrowedAttachmentIdsRef.current,
    );
    onDraftChange?.("");
    const accepted = onSubmit(submittedText, files, {
      intent:
        planSelected || command.planning
          ? "plan"
          : orchestrationSelected
            ? "orchestrate"
            : "default",
      ...(resendEdited
        ? {
            resendEdited: true,
            onResendRejected: ({ providerRewound }) => {
              if (draftRevisionRef.current !== resendDraftRevision) return;
              restoreDraft(text, files, resendBorrowedAttachmentIds);
              setResendEdited(!providerRewound);
              onEditingLastTurnChange?.(!providerRewound);
            },
          }
        : {}),
    });
    // The app can reject a turn before it is recorded (for example while an
    // orchestration is paused). Keep the user's text, files and selected mode
    // intact so resolving the blocker never destroys their work.
    if (accepted === false) {
      restoreDraft(text, files);
      return;
    }
    pasteGenerationRef.current += 1;
    if (ref.current) {
      ref.current.value = "";
      ref.current.style.height = "auto";
    }
    setDraft("");
    contextItemsRef.current = [];
    setContextItems([]);
    onDraftChange?.("");
    borrowedAttachmentIdsRef.current.clear();
    attachmentsRef.current = [];
    setAttachments([]);
    setResendEdited(false);
    onEditingLastTurnChange?.(false);
    setPlanSelected(false);
    setOperatorSelected(false);
    setOrchestrationSelected(false);
    setSessionFolderSelected(false);
    setSessionFolderOpen(false);
    setPlusOpen(false);
    setSlash(null);
    setMention(null);
    setCreatingSkill(false);
    setCreateError(null);
    setPasteError(null);
    syncHasValue("", []);
  };
  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (disabled) return;
    if (isImeComposition(e.nativeEvent)) return;
    if (creatingSkill) return;
    if (
      e.key === "Enter" &&
      !e.shiftKey &&
      consumeBtwCommand(e.currentTarget.value).matched
    ) {
      e.preventDefault();
      submit(e.currentTarget.value);
      return;
    }

    if (
      e.key === " " &&
      runsSessionFolderCommandOnSpace({
        text: e.currentTarget.value,
        selectionStart: e.currentTarget.selectionStart,
        selectionEnd: e.currentTarget.selectionEnd,
        altKey: e.altKey,
        ctrlKey: e.ctrlKey,
        metaKey: e.metaKey,
      })
    ) {
      e.preventDefault();
      openSessionFolderPicker();
      return;
    }

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
      (isCompactCommand(e.currentTarget.value) ||
        isSessionFolderCommand(e.currentTarget.value))
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

    if (
      e.key === "ArrowUp" &&
      editLastTurnSupported &&
      navigationEmpty &&
      e.currentTarget.selectionStart === 0 &&
      e.currentTarget.selectionEnd === 0
    ) {
      e.preventDefault();
      recallLastTurn();
      return;
    }

    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit(e.currentTarget.value);
    }
  };

  const onComposerKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (disabled) return;
    if (
      !draftWorkspace ||
      !onWorkspaceModeChange ||
      !enabled ||
      busy ||
      isImeComposition(e.nativeEvent) ||
      !isWorkspaceModeShortcut(e)
    ) {
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    const next =
      (workspaceMode ?? "current") === "current" ? "worktree" : "current";
    if (next === "worktree" && !resolvedWorktreeBase) return;
    onWorkspaceModeChange(
      next,
      next === "worktree" ? resolvedWorktreeBase : undefined,
    );
    ref.current?.focus();
  };

  const onPaste = (e: ClipboardEvent<HTMLTextAreaElement>) => {
    setPasteError(null);
    const messageFiles = messageFilesFromClipboard(e.clipboardData);
    if (messageFiles) {
      e.preventDefault();
      const generation = pasteGenerationRef.current;
      const captured = captureDraft(e.currentTarget);
      const text = e.clipboardData.getData("text/plain");
      if (captured) insertRestoredText(captured, text);
      if (!attachmentsSupported) return;
      rememberPaste(
        attachmentsFromFiles(messageFiles).then((pasted) => {
          if (pasteGenerationRef.current !== generation) {
            pasted.forEach(revokeAttachment);
            return;
          }
          addAttachments(pasted);
        }),
      );
      return;
    }
    const files = filesFromClipboard(e.clipboardData);
    if (files.length === 0) {
      // A webview reports a paste as text only, so a screenshot or a file
      // copied in a file manager arrives with nothing to attach; both live on
      // the native clipboard.
      if (!attachmentsSupported) return;
      const text = e.clipboardData.getData("text/plain");
      // Prose and whitespace alike are the webview's to insert.
      if (text && !isFileReferenceText(text)) return;
      // A file URI becomes a chip, so it is kept out of the draft; with no text
      // at all the paste carried an image the webview cannot see.
      e.preventDefault();
      // Captured before the read crosses an IPC hop. Send and draft reset bump
      // the generation, so a finished read cannot attach onto a draft that is gone.
      const generation = pasteGenerationRef.current;
      const captured = isFileReferenceText(text)
        ? captureDraft(e.currentTarget)
        : null;
      rememberPaste(
        nativeClipboardAttachments(text)
          .then(({ files: pasted, warning }) => {
            if (pasteGenerationRef.current !== generation) {
              pasted.forEach(revokeAttachment);
              return;
            }
            if (pasted.length) {
              // WebKit can insert the URI after preventDefault. The chip
              // replaces it, so the draft must not keep that text.
              if (captured) dropPastedText(captured, text);
              addAttachments(pasted);
            } else if (captured) insertRestoredText(captured, text);
            if (warning) setPasteError(warning);
          })
          .catch((reason: unknown) => {
            if (pasteGenerationRef.current !== generation) return;
            setPasteError(
              reason instanceof Error ? reason.message : String(reason),
            );
          }),
      );
      return;
    }
    e.preventDefault();
    if (!attachmentsSupported) return;
    const generation = pasteGenerationRef.current;
    rememberPaste(
      attachmentsFromFiles(files).then((pasted) => {
        if (pasteGenerationRef.current !== generation) {
          pasted.forEach(revokeAttachment);
          return;
        }
        addAttachments(pasted);
      }),
    );
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
      className={`relative shrink-0 ${shell || compact ? "" : "p-1.5 pt-0"}`}
      onMouseDown={disabled ? undefined : onFocus}
      onKeyDownCapture={disabled ? undefined : onComposerKeyDown}
    >
      {question && onQuestionReply ? (
        <QuestionForm
          prompt={question}
          onReply={onQuestionReply}
          onInteraction={onQuestionInteraction}
        />
      ) : null}
      {children}
      {usageLimit ? (
        <UsageLimitNotice
          limit={usageLimit}
          onResume={onUsageLimitResume}
          onResumeAtReset={onUsageLimitResumeAtReset}
          onDismiss={onUsageLimitDismiss}
        />
      ) : null}
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
        {sessionFolderOpen ? (
          <div className="absolute inset-x-0 bottom-full z-30 mb-1">
            <SessionFolderPicker
              folders={sessionFolders}
              onPick={(target) => {
                setSessionFolderOpen(false);
                setSessionFolderSelected(true);
                onPlaceInFolder?.(target);
                const el = ref.current;
                if (!el) return;
                const cursor = el.selectionStart ?? el.value.length;
                if (/^\s*\/add-to-folder$/i.test(el.value)) {
                  el.value = `${el.value} `;
                  resizeComposer(el);
                  setDraft(el.value);
                  syncHasValue(el.value, attachmentsRef.current);
                  el.setSelectionRange(el.value.length, el.value.length);
                } else {
                  el.setSelectionRange(cursor, cursor);
                }
                requestAnimationFrame(() => el.focus());
              }}
              onDismiss={() => {
                setSessionFolderOpen(false);
                ref.current?.focus();
              }}
            />
          </div>
        ) : skillPickerOpen ? (
          <div className="absolute inset-x-0 bottom-full z-30 mb-1">
            <SkillPicker
              skills={rankedSkills}
              query={slash?.query ?? ""}
              active={skillActive}
              creating={creatingSkill}
              cwd={executionCwd}
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
                void createBlankSkill({ cwd: executionCwd, name, scope })
                  .then((path) => {
                    const el = ref.current;
                    const token = slashRef.current;
                    if (el && token) {
                      const rest = el.value.slice(token.end).replace(/^\s/, "");
                      const next = `${el.value.slice(0, token.start)}${rest}`;
                      el.value = next;
                      resizeComposer(el);
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
                looksLikeProject(executionCwd) &&
                peekProjectFiles(executionCwd) == null
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
          data-composer-editing={resendEdited ? "" : undefined}
          className={`relative z-10 border bg-content/3 backdrop-blur-sm ${
            resendEdited
              ? "edit-last-turn-composer rounded-lg"
              : "rounded-lg border-content/10 has-focus:border-content/20"
          } ${
            fileDrag
              ? "border-accent/60"
              : resendEdited
                ? ""
                : "border-content/10 has-focus:border-content/20"
          }`}
        >
          {fileDrag ? (
            <div className="pointer-events-none absolute inset-0 z-20 grid place-items-center rounded-lg bg-accent/8 text-[12px] text-content/70">
              Drop files to attach
            </div>
          ) : null}
          {hideTopBar ? null : (
            <div className="flex min-w-0 items-center gap-2.5 overflow-hidden px-3 pt-2.5">
              {!remote && !hideProjectPicker ? (
                <CwdPicker
                  cwd={cwd}
                  recents={recents}
                  projectLogoPath={projectLogoPath}
                  enabled={enabled}
                  onCwdChange={onCwdChange}
                  onNewTerminal={worktreeRemoved ? undefined : onNewTerminal}
                  onClose={() => ref.current?.focus()}
                />
              ) : null}
              {hideBranchPicker ? null : draftWorkspace &&
                onWorkspaceModeChange &&
                onWorktreeBaseChange ? (
                <>
                  <WorkspacePicker
                    cwd={executionCwd}
                    mode={workspaceMode ?? "current"}
                    base={resolvedWorktreeBase}
                    enabled={enabled && !busy}
                    onModeChange={onWorkspaceModeChange}
                    onBaseChange={onWorktreeBaseChange}
                    onSelectWorktree={onWorktreeChange}
                    onOpenSettings={onManageWorktrees}
                    onClose={() => ref.current?.focus()}
                  />
                  {(workspaceMode ?? "current") === "current" ? (
                    <BranchPicker
                      cwd={executionCwd}
                      branch={branch}
                      enabled={enabled && !busy}
                      onChange={onBranchChange}
                      onClose={() => ref.current?.focus()}
                    />
                  ) : null}
                </>
              ) : worktreeRemoved && onWorktreeChange ? (
                <WorktreePicker
                  cwd={cwd}
                  executionCwd={executionCwd}
                  enabled={enabled && !busy}
                  onSelect={onWorktreeChange}
                  worktreeRemoved={worktreeRemoved}
                  onBranchChange={onBranchChange}
                  onManage={onManageWorktrees}
                  onClose={() => ref.current?.focus()}
                />
              ) : (
                <>
                  {onWorktreeChange ? (
                    <WorkspaceIdentity
                      worktree={pathKey(cwd) !== pathKey(executionCwd)}
                    />
                  ) : null}
                  <BranchPicker
                    cwd={executionCwd}
                    branch={branch}
                    enabled={enabled && !busy}
                    onChange={onBranchChange}
                    onClose={() => ref.current?.focus()}
                  />
                </>
              )}
              <div className="ml-auto flex shrink-0 items-center">
                <ContextMeter
                  usage={context}
                  onCompact={
                    compactSupported && !worktreeRemoved
                      ? onCompactContext
                      : undefined
                  }
                  compactDisabled={busy}
                />
              </div>
            </div>
          )}

          {contextItems.length > 0 || attachments.length > 0 ? (
            <div
              data-composer-context
              className="flex flex-wrap items-center gap-1.5 px-3 pt-2"
            >
              {contextItems.map((item) => (
                <ChatContextChip
                  key={chatContextKey(item)}
                  item={item}
                  animate
                  onOpenFile={onOpenFile}
                  onRemove={() => removeContextItem(item)}
                />
              ))}
              {attachments.map((file) => (
                <AttachmentChip
                  key={file.id}
                  attachment={file}
                  onRemove={() => removeAttachment(file.id)}
                />
              ))}
            </div>
          ) : null}

          {pasteError ? (
            <p role="alert" className="px-3 pt-2 text-xs text-red-400">
              {pasteError}
            </p>
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
              className={`composer-highlight pointer-events-none absolute inset-0 max-h-40 overflow-hidden whitespace-pre-wrap wrap-break-word px-3 text-sm leading-5.5 text-content font-sans ${
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
              data-composer-empty={navigationEmpty ? "true" : undefined}
              rows={1}
              spellCheck={false}
              defaultValue={initialText}
              placeholder={
                worktreeRemoved
                  ? "Select a branch or worktree to continue…"
                  : inboxCard
                    ? "Add a note, or send to start…"
                    : noteCard || contextItems.length > 0
                      ? "Add a message, or send…"
                      : handoffCard
                        ? "Add context, or send to continue…"
                        : (placeholder ??
                          (shell
                            ? "Ask, build, / for commands, @ for references... "
                            : "Ask, build, / for commands, @ for references... "))
              }
              aria-label={inputAriaLabel}
              disabled={disabled}
              className={`composer-field scrollbar-none relative max-h-40 w-full resize-none overflow-x-hidden whitespace-pre-wrap wrap-break-word bg-transparent px-3 text-sm leading-5.5 outline-none placeholder:overflow-hidden placeholder:text-ellipsis placeholder:whitespace-nowrap font-sans ${
                shell ? "py-4" : "py-3"
              }`}
              onFocus={onFocus}
              onKeyDown={onKeyDown}
              onPaste={onPaste}
              onScroll={(e) => syncHighlightScroll(e.currentTarget)}
              onClick={(e) => syncTokensFromTextarea(e.currentTarget)}
              onKeyUp={(e) => syncTokensFromTextarea(e.currentTarget)}
              onSelect={(e) => syncTokensFromTextarea(e.currentTarget)}
              onInput={(e) => {
                const el = e.currentTarget;
                if (enterBtwFromPrefix(el)) return;
                resizeComposer(el);
                draftRevisionRef.current += 1;
                setDraft(el.value);
                setPasteError(null);
                if (
                  sessionFolderSelected &&
                  !consumeSessionFolderCommand(el.value).matched
                ) {
                  setSessionFolderSelected(false);
                }
                syncHasValue(el.value, attachments);
                syncTokensFromTextarea(el);
              }}
            />
          </div>

          <div className="flex items-center gap-1 px-2 pb-2">
            <div
              ref={plusRef}
              className={compact ? "hidden" : "relative shrink-0"}
            >
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
                      <span className="block truncate whitespace-nowrap text-[11px] leading-4 text-content/45">
                        {attachmentsSupported
                          ? "Attach files or images"
                          : remote && !remoteFeatures?.attachments
                            ? "Update this machine’s host to attach files"
                            : `${HARNESS_TITLE[harness]} does not support attachments`}
                      </span>
                    </span>
                  </button>
                  {!remote || remoteFeatures?.plan ? (
                    <button
                      type="button"
                      aria-pressed={planSelected}
                      onMouseDown={(e) => e.preventDefault()}
                      onClick={() => {
                        setPlanSelected((selected) => !selected);
                        setOperatorSelected(false);
                        setOrchestrationSelected(false);
                        setDraftSelected(false);
                        setPlusOpen(false);
                        ref.current?.focus();
                      }}
                      className="flex w-full items-start gap-2.5 rounded-lg px-2 py-2 text-left text-content hover:bg-content/10"
                    >
                      <AiIdea className="mt-0.5 size-4 shrink-0 text-yellow-300/80" />
                      <span className="min-w-0 flex-1">
                        <span className="block text-[13px]">Plan mode</span>
                        <span className="block truncate whitespace-nowrap text-[11px] leading-4 text-content/45">
                          Review a plan before building
                        </span>
                      </span>
                      {planSelected ? (
                        <Check className="mt-0.5 size-3.5 shrink-0 text-accent" />
                      ) : null}
                    </button>
                  ) : null}
                  {!remote ? (
                    <button
                      type="button"
                      aria-pressed={operatorSelected}
                      onMouseDown={(event) => event.preventDefault()}
                      onClick={() => {
                        setOperatorSelected((selected) => !selected);
                        setPlanSelected(false);
                        setOrchestrationSelected(false);
                        setDraftSelected(false);
                        setPlusOpen(false);
                        ref.current?.focus();
                      }}
                      className="flex w-full items-start gap-2.5 rounded-lg px-2 py-2 text-left text-content hover:bg-content/10"
                    >
                      <CursorMagicSelection className="mt-0.5 size-4 shrink-0 text-sky-300/80" />
                      <span className="min-w-0 flex-1">
                        <span className="block text-[13px]">Operator</span>
                        <span className="block truncate whitespace-nowrap text-[11px] leading-4 text-content/45">
                          Give this thread access to MonoCode
                        </span>
                      </span>
                      {operatorSelected ? (
                        <Check className="mt-0.5 size-3.5 shrink-0 text-sky-300/80" />
                      ) : null}
                    </button>
                  ) : null}
                  {!remote && !hideTopBar && (
                    <button
                      type="button"
                      aria-pressed={orchestrationSelected}
                      onMouseDown={(event) => event.preventDefault()}
                      onClick={() => {
                        setOrchestrationSelected((selected) => !selected);
                        setPlanSelected(false);
                        setOperatorSelected(false);
                        setDraftSelected(false);
                        setPlusOpen(false);
                        ref.current?.focus();
                      }}
                      className="flex w-full items-start gap-2.5 rounded-lg px-2 py-2 text-left text-content hover:bg-content/10"
                    >
                      <Share className="mt-0.5 size-4 shrink-0 text-fuchsia-300/65" />
                      <span className="min-w-0 flex-1">
                        <span className="flex items-center gap-1.5">
                          <span className="text-[13px]">Orchestrator</span>
                          <span className="rounded-full bg-fuchsia-300/10 px-1.5 py-0.5 text-[9px] font-medium leading-none tracking-wide text-fuchsia-200/55 mb-px">
                            v1
                          </span>
                        </span>
                        <span className="block truncate whitespace-nowrap text-[11px] leading-4 text-content/45">
                          Plan and coordinate agent work
                        </span>
                      </span>
                      {orchestrationSelected && (
                        <Check className="mt-0.5 size-3.5 shrink-0 text-fuchsia-300/80" />
                      )}
                    </button>
                  )}
                  {canSaveDraft && onSaveDraft ? (
                    <button
                      type="button"
                      aria-pressed={draftSelected}
                      onMouseDown={(event) => event.preventDefault()}
                      onClick={() => {
                        setDraftSelected((selected) => !selected);
                        setPlanSelected(false);
                        setOperatorSelected(false);
                        setOrchestrationSelected(false);
                        setPlusOpen(false);
                        ref.current?.focus();
                      }}
                      className="flex w-full items-start gap-2.5 rounded-lg px-2 py-2 text-left text-content hover:bg-content/10"
                    >
                      <CircleDashed className="mt-0.5 size-4 shrink-0 text-content/60" />
                      <span className="min-w-0 flex-1">
                        <span className="block text-[13px]">Draft</span>
                        <span className="block truncate whitespace-nowrap text-[11px] leading-4 text-content/45">
                          Save this message without starting the agent
                        </span>
                      </span>
                      {draftSelected ? (
                        <Check className="mt-0.5 size-3.5 shrink-0 text-accent" />
                      ) : null}
                    </button>
                  ) : null}
                </Popover>
              ) : null}
            </div>
            {!compact && operatorSelected ? (
              <button
                type="button"
                title="Turn off Operator"
                aria-label="Turn off Operator"
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => {
                  setOperatorSelected(false);
                  ref.current?.focus();
                }}
                className="flex h-6.5 shrink-0 items-center gap-1 rounded-md bg-sky-500/15 px-1.5 text-[11px] font-medium text-sky-700 hover:bg-sky-500/20 dark:bg-sky-400/10 dark:text-sky-200/90 dark:hover:bg-sky-400/15"
              >
                <CursorMagicSelection className="size-3.5" />
                Operator
                <X className="size-3" />
              </button>
            ) : null}
            {!compact && orchestrationSelected && (
              <button
                type="button"
                title="Turn off Orchestrator mode"
                aria-label="Turn off Orchestrator mode"
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => {
                  setOrchestrationSelected(false);
                  ref.current?.focus();
                }}
                className="flex h-6.5 shrink-0 items-center gap-1 rounded-md bg-fuchsia-500/15 px-1.5 text-[11px] font-medium text-fuchsia-700 hover:bg-fuchsia-500/20 dark:bg-fuchsia-400/10 dark:text-fuchsia-200/90 dark:hover:bg-fuchsia-400/15"
              >
                <Share className="size-3.5" />
                Orchestrator
                <X className="size-3" />
              </button>
            )}
            {!compact && planSelected ? (
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
            {!compact && draftSelected ? (
              <button
                type="button"
                title="Turn off Draft mode"
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => {
                  setDraftSelected(false);
                  ref.current?.focus();
                }}
                className="flex h-6.5 shrink-0 items-center gap-1 rounded-md border border-dashed border-content/25 bg-content/5 px-1.5 text-[11px] text-content/70 hover:bg-content/10 hover:text-content"
              >
                <CircleDashed className="size-3.5" />
                Draft
                <X className="size-3" />
              </button>
            ) : null}
            <div
              className="composer-toolbar flex min-w-0 flex-1 items-center"
              onWheel={(e) => {
                if (
                  e.target instanceof Element &&
                  e.target.closest(
                    "[data-model-picker], [data-model-control], [data-access-picker], [data-model-settings]",
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
                  values={modelSettings}
                  allowedHarnesses={allowedModelHarnesses}
                  project={cwd}
                  hideSettings={controlsBeside}
                  hotkeys={hotkeys && enabled}
                  onChange={onModelChange}
                  onSettingsChange={(settings) =>
                    onModelSettingsChange?.(settings)
                  }
                  onClose={() => ref.current?.focus()}
                />
                {controlsBeside ? (
                  <ModelControlPills
                    harness={harness}
                    model={model}
                    values={modelSettings}
                    onSettingsChange={(settings) =>
                      onModelSettingsChange?.(settings)
                    }
                    onClose={() => ref.current?.focus()}
                  />
                ) : null}
                {!compact && harness !== "fx" ? (
                  <AccessPicker
                    value={runtimeMode}
                    busy={busy}
                    onChange={onRuntimeModeChange}
                    onClose={() => ref.current?.focus()}
                  />
                ) : null}
              </div>
            </div>

            {resendEdited ? (
              <button
                type="button"
                title="Stop editing last message"
                aria-label="Stop editing last message"
                onMouseDown={(event) => event.preventDefault()}
                onClick={exitEditMode}
                className="edit-last-turn-button flex h-6.5 shrink-0 items-center gap-1 rounded-md border border-current/20 px-2 text-[11px] font-medium transition-[background-color,color,border-color] hover:border-current/35 hover:bg-content/15 hover:text-content focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent"
              >
                <X className="size-3" strokeWidth={1.8} />
                <span>Cancel edit</span>
              </button>
            ) : null}
            <div className="flex shrink-0 items-center gap-1">
              <ComposerAction
                busy={busy}
                disabled={disabled}
                hasValue={hasValue && !worktreeRemoved}
                allowBusySubmit={allowBusySubmit}
                label={draftSelected ? "Save draft" : "Send"}
                onSend={() => submit(ref.current?.value ?? "")}
                onStop={() => onStop?.()}
              />
            </div>
          </div>
        </div>
        {runnerLive && runnerEnabled && !remote ? (
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

export function ComposerAction({
  busy,
  disabled = false,
  hasValue,
  allowBusySubmit = true,
  label = "Send",
  onSend,
  onStop,
}: {
  busy: boolean;
  disabled?: boolean;
  hasValue: boolean;
  allowBusySubmit?: boolean;
  label?: string;
  onSend: () => void;
  onStop: () => void;
}) {
  if (disabled) {
    return (
      <button
        type="button"
        title={label}
        aria-label={label}
        disabled
        className="composer-send primary-action grid size-6.5 place-items-center rounded-md disabled:cursor-default"
      >
        <ArrowUp className="size-3.5" strokeWidth={2.25} />
      </button>
    );
  }
  if (busy) {
    return hasValue && allowBusySubmit ? (
      <button
        type="button"
        title={label}
        aria-label={label}
        onClick={onSend}
        className="composer-send primary-action grid size-6.5 place-items-center rounded-md"
      >
        <ArrowUp className="size-3.5" strokeWidth={2.25} />
      </button>
    ) : (
      <button
        type="button"
        title="Stop"
        aria-label="Stop"
        onClick={onStop}
        className="grid size-6.5 place-items-center rounded-md bg-white text-black hover:bg-white/90"
      >
        <Square className="size-2.5 fill-current" strokeWidth={0} />
      </button>
    );
  }

  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={!hasValue}
      onClick={onSend}
      className="composer-send primary-action grid size-6.5 place-items-center rounded-md disabled:cursor-default"
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

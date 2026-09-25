import { ChevronDown, GripVertical, X } from "../../../shared/ui/icons";
import {
  memo,
  useCallback,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type CSSProperties,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { Composer } from "./Composer";
import type { Worktree } from "../../source-control/model/worktrees";
import {
  orchestrationCheckoutCwd,
  orchestrator,
  sameCheckout,
} from "../../orchestration/model/orchestration";
import { DiscussionEmpty } from "./DiscussionEmpty";
import { LinkedWorkItemUpdateNotice } from "../../inbox/ui/LinkedWorkItemUpdateNotice";
import { SessionReview } from "./SessionReview";
import { PromptOutline } from "./PromptOutline";
import {
  canCompactHarnessContext,
  type ApprovalDecision,
  type UserQuestionReply,
} from "../../../integrations/harness";
import {
  looksLikeProject,
  type RecentProject,
} from "../../projects/model/recents";
import {
  sessionDisplayTitle,
  sessionDraftBlock,
  sessionWorkCwd,
  type Attachment,
  type Block,
  type HarnessId,
  type LinkedWorkItem,
  type ModelTarget,
  type PlanBuildTarget,
  type RuntimeMode,
  type Session,
  type WorkspaceMode,
  type ComposerTurnOptions,
} from "../model/session";
import { AgentTranscript } from "./AgentTranscript";
import { PooledTranscript, type TranscriptPool } from "./TranscriptPool";
import { TranscriptFind } from "./TranscriptFind";
import {
  clearTranscriptJump,
  peekTranscriptJump,
  subscribeTranscriptJump,
} from "../model/transcriptJump";
import { EmptySession } from "./EmptySession";
import { useComposerDockMotion } from "./useComposerDockMotion";
import { MOD } from "../../../platform/tauri/platform";
import {
  acknowledgeQuoteRequest,
  ADD_TO_CHAT_EVENT,
  type AddToChatRequest,
  type QuoteRequest,
} from "../model/quoteDraft";
import { createNote, noteTitle } from "../../notes";
import {
  loadNotesEnabled,
  subscribeNotesEnabled,
} from "../../settings/model/settings";
import { getComposerDraft, setComposerDraft } from "../model/draftCache";
import { resolveModel } from "../model/models";
import { isAstraModel } from "../model/astraWelcome";
import { isOpus55Model } from "../model/opusWelcome";
import { AstraWelcome } from "./AstraWelcome";
import { OpusWelcome } from "./OpusWelcome";
import { projectKey } from "../../../shared/lib/paths";
import { canEditLastTurn, lastTurnRecall } from "../model/editLastTurn";
import {
  loadProjectChatBackgroundSettings,
  projectChatBackgroundImageRevision,
  projectChatBackgroundRevision,
  subscribeProjectChatBackground,
} from "../../projects/model/projectChatBackground";
import { useProjectBackgroundEffect } from "../../projects/ui/useProjectBackgroundEffect";
import { GradientBlurBackground } from "../../settings/ui/GradientBlurBackground";
import {
  loadChatBackgroundPath,
  loadNewThreadBackgroundEffect,
  subscribeChatBackgroundPath,
} from "../../settings/model/appearance";
import type { SessionFolderTarget } from "../model/sessionFolders";
import { markLinkedSessionUpdateSeen } from "../../inbox/model/linkedSessionSeen";

type Props = {
  session: Session;
  reviewUndoLocked?: boolean;
  visible: boolean;
  focused: boolean;
  addToChatTarget?: boolean;
  inSplit: boolean;
  composerFocused: boolean;
  composerFocusToken?: number;
  recents: RecentProject[];
  hideProjectPicker?: boolean;
  onFocus: (sessionId: string) => void;
  onClose: (sessionId: string) => void;
  onCwdChange: (sessionId: string, cwd: string) => void;
  onBranchChange: (sessionId: string) => void;
  onWorktreeChange?: (sessionId: string, tree: Worktree) => Promise<void>;
  onWorkspaceModeChange: (
    sessionId: string,
    mode: WorkspaceMode,
    base?: string,
  ) => void;
  onWorktreeBaseChange: (sessionId: string, base: string) => void;
  onManageWorktrees?: () => void;
  onModelChange: (sessionId: string, harness: HarnessId, model: string) => void;
  onModelSettingsChange: (
    sessionId: string,
    settings: Record<string, string>,
  ) => void;
  onRuntimeModeChange: (sessionId: string, mode: RuntimeMode) => void;
  onSubmit: (
    sessionId: string,
    text: string,
    attachments: Attachment[],
    options?: ComposerTurnOptions,
  ) => boolean | void;
  onSaveDraft: (
    sessionId: string,
    text: string,
    attachments: Attachment[],
  ) => boolean | void;
  onRemoveDraft: (sessionId: string, draftBlockId: string) => boolean | void;
  onStop: (sessionId: string) => void;
  onCompactContext: (sessionId: string) => boolean;
  onPlaceSessionInFolder: (
    sessionId: string,
    target: SessionFolderTarget,
  ) => void;
  onDeleteQueuedMessage: (sessionId: string, messageId: string) => void;
  onEditQueuedMessage: (
    sessionId: string,
    messageId: string,
    text: string,
  ) => void;
  onQueuedMessageEditingChange: (sessionId: string, messageId?: string) => void;
  onSteerQueuedMessage: (sessionId: string, messageId: string) => void;
  onResumeQueue: (sessionId: string) => void;
  onInboxCardDismiss?: (sessionId: string) => void;
  onLinkedWorkItemUpdateCardDismiss?: (sessionId: string) => void;
  onNoteCardDismiss?: (sessionId: string) => void;
  onHandoffCardDismiss?: (sessionId: string) => void;
  onOpenLinkedWorkItem?: (item: LinkedWorkItem, sessionId: string) => void;
  onArchiveSession?: (sessionId: string, archived: boolean) => Promise<boolean>;
  onDeleteSession?: (sessionId: string) => Promise<boolean>;
  onApproval: (
    sessionId: string,
    requestId: number,
    decision: ApprovalDecision,
  ) => void;
  onQuestionReply: (
    sessionId: string,
    requestId: number,
    reply: UserQuestionReply,
  ) => void;
  onQuestionInteraction?: (sessionId: string, requestId: number) => void;
  onOpenFile: (path: string) => void;
  onOpenDiff: (
    path?: string,
    session?: { sessionId: string; cwd: string },
  ) => void;
  onOpenPlan: (sessionId: string, blockId: string) => void;
  onBuildPlan: (
    sessionId: string,
    blockId: string,
    target?: PlanBuildTarget,
  ) => void;
  onSecondOpinion?: (
    sessionId: string,
    target: ModelTarget,
    turn: Block[],
  ) => void;
  onHandoff?: (sessionId: string, target: ModelTarget, turn: Block[]) => void;
  onNewTerminal: (sessionId: string) => void;
  onPaneDragStart?: (event: ReactPointerEvent<HTMLElement>) => void;
  /** Keeps this transcript mounted after the pane closes. */
  transcriptPool?: TranscriptPool;
};

export const SessionPane = memo(function SessionPane({
  session,
  reviewUndoLocked = false,
  visible,
  focused,
  addToChatTarget = focused,
  inSplit,
  composerFocused,
  composerFocusToken,
  recents,
  hideProjectPicker,
  onFocus,
  onClose,
  onCwdChange,
  onBranchChange,
  onWorktreeChange,
  onWorkspaceModeChange,
  onWorktreeBaseChange,
  onManageWorktrees,
  onModelChange,
  onModelSettingsChange,
  onRuntimeModeChange,
  onSaveDraft,
  onRemoveDraft,
  onSubmit,
  onStop,
  onCompactContext,
  onPlaceSessionInFolder,
  onDeleteQueuedMessage,
  onEditQueuedMessage,
  onQueuedMessageEditingChange,
  onSteerQueuedMessage,
  onResumeQueue,
  onInboxCardDismiss,
  onLinkedWorkItemUpdateCardDismiss,
  onNoteCardDismiss,
  onHandoffCardDismiss,
  onOpenLinkedWorkItem,
  onArchiveSession,
  onDeleteSession,
  onApproval,
  onQuestionReply,
  onQuestionInteraction,
  onOpenFile,
  onOpenDiff,
  onOpenPlan,
  onBuildPlan,
  onSecondOpinion,
  onHandoff,
  onNewTerminal,
  onPaneDragStart,
  transcriptPool,
}: Props) {
  const orchestrationRuns = useSyncExternalStore(
    orchestrator.subscribe,
    orchestrator.snapshot,
    orchestrator.snapshot,
  );
  const managed = orchestrationRuns.some(
    (run) =>
      (run.status === "active" || run.status === "paused") &&
      sameCheckout(orchestrationCheckoutCwd(run), sessionWorkCwd(session)),
  );
  const title = sessionDisplayTitle(session.title, session.harness);
  const isEmpty = session.blocks.length === 0;
  const recallLastTurnRef = useRef<(() => void) | null>(null);
  const editLastTurnSupported = canEditLastTurn(session);
  const turnRecall = editLastTurnSupported ? lastTurnRecall(session) : null;
  const draftBlock = sessionDraftBlock(session);
  useSyncExternalStore(
    subscribeProjectChatBackground,
    projectChatBackgroundRevision,
    projectChatBackgroundRevision,
  );
  const globalBackgroundPath = useSyncExternalStore(
    subscribeChatBackgroundPath,
    loadChatBackgroundPath,
    loadChatBackgroundPath,
  );
  const globalBackgroundEffect = useSyncExternalStore(
    subscribeChatBackgroundPath,
    loadNewThreadBackgroundEffect,
    loadNewThreadBackgroundEffect,
  );
  const projectBackground = loadProjectChatBackgroundSettings(
    projectKey(session.cwd),
  );
  const projectBackgroundUrl = useProjectBackgroundEffect(
    projectBackground?.path ?? null,
    projectBackground?.effect ?? "none",
    projectChatBackgroundImageRevision(),
  );
  const projectBackgroundStyle = projectBackground
    ? ({
        "--chat-background-image": projectBackgroundUrl
          ? `url(${JSON.stringify(projectBackgroundUrl)})`
          : "none",
        "--chat-background-empty-opacity": String(
          projectBackground.emptyOpacity,
        ),
        "--chat-background-session-opacity": String(
          projectBackground.sessionOpacity,
        ),
      } as CSSProperties)
    : undefined;
  const approve = useCallback(
    (requestId: number, decision: ApprovalDecision) =>
      onApproval(session.id, requestId, decision),
    [onApproval, session.id],
  );
  const replyQuestion = useCallback(
    (requestId: number, reply: UserQuestionReply) =>
      onQuestionReply(session.id, requestId, reply),
    [onQuestionReply, session.id],
  );
  const openPlan = useCallback(
    (blockId: string) => onOpenPlan(session.id, blockId),
    [onOpenPlan, session.id],
  );
  const buildPlan = useCallback(
    (blockId: string, target?: PlanBuildTarget) =>
      onBuildPlan(session.id, blockId, target),
    [onBuildPlan, session.id],
  );
  const jumpToBottomRef = useRef<(() => void) | null>(null);
  const transcriptScope = useRef<HTMLDivElement>(null);
  const [transcriptScroller, setTranscriptScroller] =
    useState<HTMLDivElement | null>(null);
  const focusPane = useCallback(
    () => onFocus(session.id),
    [onFocus, session.id],
  );
  const quoteRequestId = useRef(0);
  const [showJumpToBottom, setShowJumpToBottom] = useState(false);
  const [editingLastTurn, setEditingLastTurn] = useState(false);
  useEffect(() => {
    setEditingLastTurn(false);
  }, [session.id, editLastTurnSupported]);
  const modelWelcomeSequence = useRef(0);
  const [modelWelcome, setModelWelcome] = useState<{
    kind: "astra" | "opus";
    run: number;
  } | null>(null);
  const dismissModelWelcome = useCallback(() => setModelWelcome(null), []);
  useEffect(() => {
    if (!visible) setModelWelcome(null);
  }, [visible]);
  // Restore a saved run for this lead; its agents render on the sidebar card.
  useEffect(() => {
    if (!session.inboxAsk && !session.worktreeRemoved)
      void orchestrator.hydrate(session.id).catch(console.error);
  }, [session.id, session.inboxAsk, session.worktreeRemoved]);
  const [quoteRequest, setQuoteRequest] = useState<QuoteRequest>();
  const onJumpToBottomReady = useCallback((jump: () => void) => {
    jumpToBottomRef.current = jump;
  }, []);
  const revealBlockRef = useRef<((blockId: string) => boolean) | null>(null);
  const onRevealReady = useCallback((reveal: (blockId: string) => boolean) => {
    revealBlockRef.current = reveal;
  }, []);
  const revealBlock = useCallback(
    (blockId: string) => revealBlockRef.current?.(blockId) ?? false,
    [],
  );
  const navigateBlockRef = useRef<
    ((blockId: string | null, query?: string) => boolean) | null
  >(null);
  const [navigatorReady, setNavigatorReady] = useState(false);
  const onNavigateReady = useCallback(
    (navigate: (blockId: string | null, query?: string) => boolean) => {
      navigateBlockRef.current = navigate;
      setNavigatorReady(true);
    },
    [],
  );
  const navigateBlock = useCallback(
    (blockId: string | null, query?: string) =>
      navigateBlockRef.current?.(blockId, query) ?? false,
    [],
  );
  const jumpRequest = useSyncExternalStore(
    subscribeTranscriptJump,
    () => peekTranscriptJump(session.id),
    () => null,
  );
  useEffect(() => {
    if (!visible || !navigatorReady || !jumpRequest) return;
    const frame = requestAnimationFrame(() => {
      if (navigateBlock(jumpRequest.blockId, jumpRequest.query)) {
        clearTranscriptJump(session.id, jumpRequest.token);
      }
    });
    return () => cancelAnimationFrame(frame);
  }, [visible, navigatorReady, jumpRequest, navigateBlock, session.id]);
  const addSelectionToChat = useCallback(
    (text: string, mode?: QuoteRequest["mode"]) => {
      quoteRequestId.current += 1;
      setQuoteRequest({ id: quoteRequestId.current, text, mode });
    },
    [],
  );
  const acknowledgeQuote = useCallback((handledId: number) => {
    setQuoteRequest((current) => acknowledgeQuoteRequest(current, handledId));
  }, []);
  const notesEnabled = useSyncExternalStore(
    subscribeNotesEnabled,
    loadNotesEnabled,
    () => true,
  );
  const saveNote = useCallback(
    async (text: string) => {
      const sessionTitle = sessionDisplayTitle(session.title, session.harness);
      await createNote({
        title:
          sessionTitle && sessionTitle !== "New session"
            ? sessionTitle
            : noteTitle(text),
        body: text,
        sourceSessionId: session.id,
        sourceCwd: session.cwd,
      });
    },
    [session.cwd, session.harness, session.id, session.title],
  );
  const saveSelectionNote = useCallback(
    async (text: string) => {
      await createNote({
        title: noteTitle(text),
        body: text,
        sourceSessionId: session.id,
        sourceCwd: session.cwd,
      });
    },
    [session.cwd, session.id],
  );

  useEffect(() => {
    if (!addToChatTarget) return;
    const onAdd = (event: Event) => {
      const detail = (event as CustomEvent<AddToChatRequest>).detail;
      if (!detail?.text) return;
      addSelectionToChat(detail.text, detail.mode);
    };
    window.addEventListener(ADD_TO_CHAT_EVENT, onAdd);
    return () => window.removeEventListener(ADD_TO_CHAT_EVENT, onAdd);
  }, [addSelectionToChat, addToChatTarget]);
  const workCwd = sessionWorkCwd(session);
  const showDeckProjectPicker = isEmpty && !looksLikeProject(session.cwd);
  const dockComposer =
    !draftBlock && (!isEmpty || inSplit || !!session.inboxAsk);
  const composerDockMotion = useComposerDockMotion(dockComposer);
  const draftRef = useRef<string | undefined>(getComposerDraft(session.id));
  const composer = (
    <Composer
      enabled={visible}
      focused={focused && composerFocused}
      focusToken={composerFocusToken}
      hotkeys={focused}
      shell={!dockComposer}
      harness={session.harness}
      model={session.model}
      modelSettings={session.modelSettings}
      runtimeMode={session.runtimeMode}
      cwd={session.cwd}
      executionCwd={workCwd}
      sessionId={session.id}
      compactSupported={canCompactHarnessContext(session.harness)}
      recents={recents}
      hideProjectPicker={
        !!session.inboxAsk ||
        (hideProjectPicker ? !showDeckProjectPicker : false)
      }
      hideBranchPicker={!!session.inboxAsk || managed}
      hideTopBar={!!session.inboxAsk}
      context={session.context}
      quoteRequest={quoteRequest}
      initialDraft={
        draftRef.current ??
        (session.inboxCard || session.noteCard || session.handoffCard
          ? undefined
          : session.composerSeed)
      }
      onDraftChange={(text) => {
        draftRef.current = text;
        setComposerDraft(session.id, text);
      }}
      inboxCard={session.inboxCard}
      noteCard={session.noteCard}
      handoffCard={session.handoffCard}
      question={session.pendingQuestion}
      onQuoteRequestConsumed={acknowledgeQuote}
      onInboxCardDismiss={() => onInboxCardDismiss?.(session.id)}
      onNoteCardDismiss={() => onNoteCardDismiss?.(session.id)}
      onHandoffCardDismiss={() => onHandoffCardDismiss?.(session.id)}
      onQuestionReply={replyQuestion}
      onQuestionInteraction={(id) => onQuestionInteraction?.(session.id, id)}
      onFocus={() => onFocus(session.id)}
      onCwdChange={(cwd) => onCwdChange(session.id, cwd)}
      onBranchChange={() => onBranchChange(session.id)}
      onWorktreeChange={
        onWorktreeChange
          ? (tree) => onWorktreeChange(session.id, tree)
          : undefined
      }
      draftWorkspace={
        !session.inboxAsk &&
        !session.worktreeRemoved &&
        !managed &&
        ((isEmpty && !session.worktreeCwd) ||
          (!!session.workspaceMode && !session.worktreeCwd))
      }
      workspaceMode={session.workspaceMode}
      worktreeBase={session.worktreeBase}
      onWorkspaceModeChange={(mode, base) =>
        onWorkspaceModeChange(session.id, mode, base)
      }
      onWorktreeBaseChange={(base) => onWorktreeBaseChange(session.id, base)}
      worktreeRemoved={session.worktreeRemoved}
      onManageWorktrees={onManageWorktrees}
      onNewTerminal={() => onNewTerminal(session.id)}
      onModelChange={(harness, model) => {
        onModelChange(session.id, harness, model);
        const selected = resolveModel(harness, model);
        // A new key restarts the animation and its cleanup timer on every pick.
        const kind = isAstraModel(selected)
          ? "astra"
          : isOpus55Model(selected)
            ? "opus"
            : null;
        setModelWelcome(kind && { kind, run: ++modelWelcomeSequence.current });
      }}
      onModelSettingsChange={(settings) =>
        onModelSettingsChange(session.id, settings)
      }
      onRuntimeModeChange={(mode) => onRuntimeModeChange(session.id, mode)}
      canSaveDraft={
        !session.busy &&
        !draftBlock &&
        !session.inboxAsk &&
        !session.inboxCard &&
        !session.noteCard &&
        !session.handoffCard
      }
      onSaveDraft={(text, attachments) =>
        onSaveDraft(session.id, text, attachments)
      }
      onSubmit={(text, attachments, options) => {
        if (!dockComposer) composerDockMotion.captureLaunch();
        return onSubmit(session.id, text, attachments, options);
      }}
      onStop={() => onStop(session.id)}
      onCompactContext={() => onCompactContext(session.id)}
      onPlaceInFolder={(target) => onPlaceSessionInFolder(session.id, target)}
      queuedMessages={session.queuedMessages}
      queueStatus={session.queueStatus}
      onDeleteQueuedMessage={(messageId) =>
        onDeleteQueuedMessage(session.id, messageId)
      }
      onEditQueuedMessage={(messageId, text) =>
        onEditQueuedMessage(session.id, messageId, text)
      }
      onQueuedMessageEditingChange={(messageId) =>
        onQueuedMessageEditingChange(session.id, messageId)
      }
      onSteerQueuedMessage={(messageId) =>
        onSteerQueuedMessage(session.id, messageId)
      }
      onResumeQueue={() => onResumeQueue(session.id)}
      onOpenFile={onOpenFile}
      busy={!!session.busy}
      editLastTurnSupported={editLastTurnSupported}
      lastTurnRecall={turnRecall}
      onRecallLastTurnReady={(recall) => {
        recallLastTurnRef.current = recall;
      }}
      onEditingLastTurnChange={setEditingLastTurn}
    />
  );

  return (
    <div
      data-session-drop={session.id}
      data-session-empty={isEmpty}
      data-project-chat-background={!!projectBackground}
      data-project-background-effect={projectBackground?.effect}
      data-project-background-scope={projectBackground?.scope}
      style={projectBackgroundStyle}
      className="chat-pane-background relative isolate flex h-full min-h-0 min-w-0 flex-1 flex-col"
      onMouseDown={() => onFocus(session.id)}
    >
      {projectBackground?.effect === "gradient-blur" ||
      (!projectBackground &&
        globalBackgroundPath &&
        globalBackgroundEffect === "gradient-blur") ? (
        <GradientBlurBackground />
      ) : null}
      {modelWelcome && visible ? (
        modelWelcome.kind === "astra" ? (
          <AstraWelcome key={modelWelcome.run} onDone={dismissModelWelcome} />
        ) : (
          <OpusWelcome key={modelWelcome.run} onDone={dismissModelWelcome} />
        )
      ) : null}
      {inSplit ? (
        <div
          className={`flex h-9 shrink-0 touch-none items-center gap-1.5 border-b border-stroke px-2 select-none ${
            onPaneDragStart ? "cursor-grab active:cursor-grabbing" : ""
          }`}
          onPointerDown={(event) => {
            if (event.button !== 0 || !onPaneDragStart) return;
            if (
              (event.target as HTMLElement | null)?.closest("[data-no-drag]")
            ) {
              return;
            }
            onPaneDragStart(event);
          }}
        >
          {onPaneDragStart ? (
            <GripVertical
              className="size-3.5 shrink-0 text-content/35"
              strokeWidth={1.75}
            />
          ) : null}
          <span
            className={`size-2 shrink-0 rounded-full ${focused ? "bg-accent" : "bg-transparent"}`}
          />
          <span
            className="min-w-0 flex-1 truncate text-xs text-content"
            title={title}
          >
            {title}
          </span>
          <button
            type="button"
            title={`Close Pane (${MOD}W)`}
            aria-label="Close pane"
            data-no-drag
            className="grid size-5 shrink-0 place-items-center rounded text-content/50 hover:bg-content/10 hover:text-content"
            onPointerDown={(e) => e.stopPropagation()}
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              onClose(session.id);
            }}
          >
            <X className="size-3" strokeWidth={1.75} />
          </button>
        </div>
      ) : null}
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <div
          ref={transcriptScope}
          className="@container relative min-h-0 flex-1"
        >
          {visible && focused && !session.inboxAsk ? (
            <LinkedWorkItemUpdateNotice
              sessionId={session.id}
              card={session.linkedWorkItemUpdateCard}
              onAcknowledge={() => {
                const updatedAt = session.linkedWorkItemUpdateCard?.updatedAt;
                if (updatedAt != null) {
                  markLinkedSessionUpdateSeen(session.id, updatedAt);
                }
              }}
              onDismiss={() => onLinkedWorkItemUpdateCardDismiss?.(session.id)}
              onOpenDiscussion={() => {
                if (session.linkedWorkItem) {
                  onOpenLinkedWorkItem?.(session.linkedWorkItem, session.id);
                }
              }}
              onAddToChat={(text) => addSelectionToChat(text, "plain")}
              onArchiveSession={
                onArchiveSession
                  ? () => onArchiveSession(session.id, true)
                  : undefined
              }
              onDeleteSession={
                onDeleteSession ? () => onDeleteSession(session.id) : undefined
              }
            />
          ) : null}
          {isEmpty ? (
            session.inboxAsk ? (
              <div className="scrollbar-none h-full min-h-0 overflow-y-auto">
                <DiscussionEmpty message="Explore this item with your agent." />
              </div>
            ) : (
              <EmptySession
                cwd={session.cwd}
                hasChatBackground={Boolean(
                  projectBackground || globalBackgroundPath,
                )}
                composer={
                  dockComposer ? undefined : (
                    <div
                      ref={composerDockMotion.centeredRef}
                      data-session-composer
                    >
                      {composer}
                    </div>
                  )
                }
              />
            )
          ) : (
            <>
              <PooledTranscript
                pool={transcriptPool}
                sessionId={session.id}
                onMouseDown={focusPane}
              >
                <AgentTranscript
                  blocks={session.blocks}
                  busy={!!session.busy}
                  visible={visible}
                  cwd={workCwd}
                  harness={session.harness}
                  model={session.model}
                  modelSettings={session.modelSettings}
                  pendingQuestion={!!session.pendingQuestion}
                  backgroundTasks={session.backgroundTasks}
                  onApproval={session.worktreeRemoved ? undefined : approve}
                  onAddToChat={addSelectionToChat}
                  onSaveNote={notesEnabled ? saveNote : undefined}
                  onSendDraft={
                    draftBlock
                      ? (block) =>
                          onSubmit(
                            session.id,
                            block.text,
                            block.attachments ?? [],
                            {
                              draftBlockId: block.id,
                              ...(block.appRequestId
                                ? { appRequestId: block.appRequestId }
                                : {}),
                            },
                          )
                      : undefined
                  }
                  onRemoveDraft={
                    draftBlock
                      ? (block) => onRemoveDraft(session.id, block.id)
                      : undefined
                  }
                  onSaveSelectionNote={
                    notesEnabled ? saveSelectionNote : undefined
                  }
                  onOpenFile={onOpenFile}
                  onOpenDiff={onOpenDiff}
                  onOpenPlan={openPlan}
                  onBuildPlan={session.worktreeRemoved ? undefined : buildPlan}
                  onSecondOpinion={
                    !session.inboxAsk &&
                    !session.worktreeRemoved &&
                    onSecondOpinion
                      ? (target, turn) =>
                          onSecondOpinion(session.id, target, turn)
                      : undefined
                  }
                  onHandoff={
                    !session.inboxAsk && !session.worktreeRemoved && onHandoff
                      ? (target, turn) => onHandoff(session.id, target, turn)
                      : undefined
                  }
                  onJumpToBottomChange={setShowJumpToBottom}
                  onJumpToBottomReady={onJumpToBottomReady}
                  onRevealReady={onRevealReady}
                  onNavigateReady={onNavigateReady}
                  onScrollerChange={setTranscriptScroller}
                  editingLastTurn={editingLastTurn}
                  onEditLastTurn={
                    editLastTurnSupported
                      ? () => {
                          onFocus(session.id);
                          recallLastTurnRef.current?.();
                        }
                      : undefined
                  }
                  latestTurnAccessory={
                    session.inboxAsk ||
                    session.worktreeRemoved ||
                    draftBlock ? undefined : (
                      <SessionReview
                        sessionId={session.id}
                        cwd={workCwd}
                        enabled={visible}
                        busy={!!session.busy}
                        undoLocked={
                          reviewUndoLocked ||
                          orchestrationRuns.some(
                            (run) =>
                              (run.status === "active" ||
                                run.status === "paused") &&
                              (run.leadId === session.id ||
                                run.tasks.some(
                                  (task) => task.sessionId === session.id,
                                )),
                          )
                        }
                        onOpenDiff={onOpenDiff}
                      />
                    )
                  }
                />
              </PooledTranscript>
              {!session.inboxAsk ? (
                <TranscriptFind
                  blocks={session.blocks}
                  visible={visible}
                  focused={focused}
                  onNavigate={navigateBlock}
                  side={
                    session.linkedWorkItemUpdateCard &&
                    session.linkedWorkItemUpdateCard.status !== "loading"
                      ? "left"
                      : "right"
                  }
                />
              ) : null}
              <PromptOutline
                blocks={session.blocks}
                scope={transcriptScope}
                scroller={transcriptScroller}
                visible={visible}
                revealBlock={revealBlock}
              />
              {showJumpToBottom ? (
                <div className="pointer-events-none absolute inset-x-0 bottom-2 z-30 flex justify-center">
                  <button
                    type="button"
                    title="Jump to latest"
                    aria-label="Jump to latest"
                    data-jump-to-bottom
                    onClick={() => jumpToBottomRef.current?.()}
                    className="pointer-events-auto grid size-6 place-items-center rounded-md border border-content/15 bg-content/10 text-content shadow-md hover:bg-content/5 backdrop-blur-md"
                  >
                    <ChevronDown className="size-4" strokeWidth={2} />
                  </button>
                </div>
              ) : null}
            </>
          )}
        </div>
        {dockComposer ? (
          <div
            ref={composerDockMotion.dockedRef}
            data-session-composer
            className="mx-auto w-full max-w-4xl shrink-0"
          >
            {composer}
          </div>
        ) : null}
      </div>
    </div>
  );
});

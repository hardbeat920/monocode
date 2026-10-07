import { GripVertical, X } from "../../../shared/ui/icons";
import {
  memo,
  useCallback,
  useEffect,
  useMemo,
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
import { sessionHasBtwThreads, supportsBtwHarness } from "../model/btw";
import { BtwSheet, useBtwConversation } from "./BtwSheet";
import { AgentTranscript } from "./AgentTranscript";
import { PooledTranscript, type TranscriptPool } from "./TranscriptPool";
import { TranscriptFind } from "./TranscriptFind";
import {
  TranscriptJumpToBottom,
  useTranscriptJumpVisibility,
} from "./TranscriptJumpToBottom";
import {
  clearTranscriptJump,
  peekTranscriptJump,
  subscribeTranscriptJump,
} from "../model/transcriptJump";
import { EmptySession } from "./EmptySession";
import {
  monoForSession,
  monoLook,
  monosSnapshot,
  subscribeMonos,
} from "../../monos/model/mono";
import { MonoHeader } from "../../monos/ui/MonoHeader";
import { MonoComposer } from "../../monos/ui/MonoComposer";
import { MonoUsageLimitNotice } from "../../monos/ui/MonoUsageLimitNotice";
import { QuestionForm } from "./QuestionForm";
import {
  monoMessageDeliveries,
  monoPendingTranscriptBlocks,
} from "../../monos/model/monoMessaging";
import { MessageQueue } from "./MessageQueue";
import { useMonoTranscript } from "../../monos/hooks/useMonoTranscript";
import { MONO_PAGE_TURNS } from "../data/sessionStore";
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
import { monoChatBackground } from "../../monos/model/monoBackground";
import { GradientBlurBackground } from "../../settings/ui/GradientBlurBackground";
import {
  loadChatBackgroundPath,
  loadNewThreadBackgroundEffect,
  subscribeChatBackgroundPath,
} from "../../settings/model/appearance";
import type { SessionFolderTarget } from "../model/sessionFolders";
import { markLinkedSessionUpdateSeen } from "../../inbox/model/linkedSessionSeen";
import { RemoteSession } from "../../connections/ui/RemoteSession";
import { isRemoteProjectPath } from "../../projects/model/recents";
import type { HostSession } from "../../connections/model/protocol";

export type SessionPaneProps = {
  session: Session;
  workspaceSwitchingSessionId?: string;
  reviewUndoLocked?: boolean;
  visible: boolean;
  focused: boolean;
  addToChatTarget?: boolean;
  inSplit: boolean;
  composerFocused: boolean;
  composerFocusToken?: number;
  onShowMonoActivity?: (
    sessionId: string,
    turnId: string,
    blocks: Block[],
  ) => void;
  monoActivityTurnId?: string;
  onShowMonoSessions?: (
    sessionId: string,
    turnId: string,
    blocks: Block[],
  ) => void;
  monoSessionsTurnId?: string;
  recents: RecentProject[];
  hideProjectPicker?: boolean;
  onFocus: (sessionId: string) => void;
  onClose: (sessionId: string) => void;
  onCwdChange: (sessionId: string, cwd: string) => void;
  onBranchChange: (sessionId: string) => void;
  onWorktreeChange?: (sessionId: string, tree: Worktree) => Promise<void>;
  onRemoteSnapshot?: (shellId: string, snapshot?: HostSession) => void;
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
  onUsageLimitResume: (sessionId: string) => void;
  onUsageLimitResumeAtReset: (sessionId: string, enabled: boolean) => void;
  onUsageLimitDismiss: (sessionId: string) => void;
  onUsageLimitAccountChange?: (sessionId: string, accountId: string) => void;
  onInboxCardDismiss?: (sessionId: string) => void;
  onLinkedWorkItemUpdateCardDismiss?: (sessionId: string) => void;
  onNoteCardDismiss?: (sessionId: string) => void;
  onOpenArtifact?: (sessionId: string, id: string) => void;
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
  onBtwSubmit?: (
    sessionId: string,
    turn: Block[],
    threadId: string,
    messageId: string,
    text: string,
    model?: string,
    modelSettings?: Record<string, string>,
  ) => boolean | void;
  onBtwRetry?: (sessionId: string, turn: Block[], threadId: string) => void;
  onBtwDelete?: (sessionId: string, turn: Block[], threadId: string) => void;
  onBtwStop?: (sessionId: string, turn: Block[], threadId: string) => void;
  onBtwModelChange?: (
    sessionId: string,
    turn: Block[],
    threadId: string,
    model: string,
    modelSettings: Record<string, string>,
  ) => void;
  onNewTerminal: (sessionId: string) => void;

  onPaneDragStart?: (event: ReactPointerEvent<HTMLElement>) => void;
  /** Keeps this transcript mounted after the pane closes. */
  transcriptPool?: TranscriptPool;
};

type Props = SessionPaneProps & {
  /** The session runtime is on another machine. */
  remoteSession?: boolean;
  remoteFeatures?: { attachments: boolean; plan: boolean; draft: boolean };
  /** An opened host conversation whose transcript has not arrived yet. */
  remoteSessionLoading?: boolean;
  remoteSessionStarted?: boolean;
  allowedModelHarnesses?: readonly HarnessId[];
};

export const SessionPane = memo(function SessionPane(props: SessionPaneProps) {
  // Sessions in a project on another machine render this same pane, backed by
  // the host instead of this computer's session runtime.
  if (isRemoteProjectPath(props.session.cwd))
    return (
      <RemoteSession
        shell={props.session}
        visible={props.visible}
        onSnapshot={props.onRemoteSnapshot}
        onOpenFile={props.onOpenFile}
        onOpenDiff={props.onOpenDiff}
        onOpenPlan={props.onOpenPlan}
        render={(remote) => <LocalSessionPane {...props} {...remote} />}
      />
    );
  return <LocalSessionPane {...props} />;
});

const LocalSessionPane = memo(function LocalSessionPane({
  remoteSession = false,
  remoteFeatures,
  remoteSessionLoading = false,
  remoteSessionStarted = false,
  allowedModelHarnesses,
  session,
  workspaceSwitchingSessionId,
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
  onUsageLimitResume,
  onUsageLimitResumeAtReset,
  onUsageLimitDismiss,
  onUsageLimitAccountChange,
  onInboxCardDismiss,
  onLinkedWorkItemUpdateCardDismiss,
  onNoteCardDismiss,
  onOpenArtifact,
  onHandoffCardDismiss,
  onOpenLinkedWorkItem,
  onArchiveSession,
  onDeleteSession,
  onApproval,
  onQuestionReply,
  onQuestionInteraction,
  onOpenFile,
  onOpenDiff,
  onShowMonoActivity,
  monoActivityTurnId,
  onShowMonoSessions,
  monoSessionsTurnId,
  onOpenPlan,
  onBuildPlan,
  onSecondOpinion,
  onHandoff,
  onBtwSubmit,
  onBtwRetry,
  onBtwDelete,
  onBtwStop,
  onBtwModelChange,
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
  const messageDeliveries = useMemo(
    () => monoMessageDeliveries(session),
    [session.queuedMessages, session.queueStatus],
  );
  const recallLastTurnRef = useRef<(() => void) | null>(null);
  const remote = remoteSession;
  // A Mono's chat is a running conversation; what was said stays said.
  const editLastTurnSupported =
    !remote && !monoForSession(session.id) && canEditLastTurn(session);
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
  // Renames, instructions and mascot picks re-render the agent's look.
  useSyncExternalStore(subscribeMonos, monosSnapshot);
  const mono = monoForSession(session.id);
  const agent = mono ? monoLook(mono) : undefined;
  // A Mono shows only its own background, never the project's or the app's.
  const projectBackground = mono
    ? monoChatBackground(mono.id)
    : loadProjectChatBackgroundSettings(projectKey(session.cwd));
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
  const jumpVisibility = useTranscriptJumpVisibility();
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
    if (!remote && !session.inboxAsk && !session.worktreeRemoved)
      void orchestrator.hydrate(session.id).catch(console.error);
  }, [remote, session.id, session.inboxAsk, session.worktreeRemoved]);
  const [quoteRequest, setQuoteRequest] = useState<QuoteRequest>();
  const btw = useBtwConversation({
    available:
      !remote &&
      !isEmpty &&
      !managed &&
      !session.inboxAsk &&
      !session.worktreeRemoved &&
      !!onBtwSubmit &&
      !!onBtwRetry &&
      (supportsBtwHarness(session.harness) ||
        sessionHasBtwThreads(session.blocks)),
    blocks: session.blocks,
    harness: session.harness,
    managed,
    model: session.model,
    modelSettings: session.modelSettings,
    onSubmit: (turn, threadId, messageId, text, model, modelSettings) =>
      onBtwSubmit?.(
        session.id,
        turn,
        threadId,
        messageId,
        text,
        model,
        modelSettings,
      ),
    onRetry: (turn, threadId) => onBtwRetry?.(session.id, turn, threadId),
    onDelete: (turn, threadId) => onBtwDelete?.(session.id, turn, threadId),
    onStop: (turn, threadId) => onBtwStop?.(session.id, turn, threadId),
    onModelChange: (turn, threadId, model, modelSettings) =>
      onBtwModelChange?.(session.id, turn, threadId, model, modelSettings),
  });
  const onJumpToBottomReady = useCallback((jump: () => void) => {
    jumpToBottomRef.current = jump;
  }, []);
  const monoTranscript = useMonoTranscript(
    session,
    !!monoForSession(session.id),
  );
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
    async (blockId: string | null, query?: string) => {
      if (navigateBlockRef.current?.(blockId, query)) return true;
      if (!blockId) return false;
      if (!(await monoTranscript.reveal(blockId))) return false;
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => resolve()),
      );
      return navigateBlockRef.current?.(blockId, query) ?? false;
    },
    [monoTranscript.reveal],
  );
  const jumpRequest = useSyncExternalStore(
    subscribeTranscriptJump,
    () => peekTranscriptJump(session.id),
    () => null,
  );
  useEffect(() => {
    if (!visible || !navigatorReady || !jumpRequest) return;
    let cancelled = false;
    const frame = requestAnimationFrame(() => {
      void navigateBlock(jumpRequest.blockId, jumpRequest.query)
        .then((found) => {
          if (found && !cancelled)
            clearTranscriptJump(session.id, jumpRequest.token);
        })
        .catch(() => undefined);
    });
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
    };
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
  // The agent's input always sits at the bottom, like a chat.
  const dockComposer =
    remoteSessionLoading ||
    (!draftBlock && (!isEmpty || inSplit || !!session.inboxAsk || !!agent));
  const composerDockMotion = useComposerDockMotion(dockComposer);
  const draftRef = useRef<string | undefined>(getComposerDraft(session.id));
  const composer = (
    <Composer
      key={session.id}
      disabled={workspaceSwitchingSessionId === session.id}
      remoteSession={remoteSession}
      remoteFeatures={remoteFeatures}
      allowedModelHarnesses={allowedModelHarnesses}
      enabled={visible}
      focused={focused && composerFocused && !btw.open}
      focusToken={composerFocusToken}
      hotkeys={focused && !btw.open}
      shell={!dockComposer}
      harness={session.harness}
      model={session.model}
      modelSettings={session.modelSettings}
      runtimeMode={session.runtimeMode}
      cwd={session.cwd}
      executionCwd={workCwd}
      sessionId={session.id}
      providerAccountId={session.providerAccountId}
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
        (remote
          ? !remoteSessionStarted
          : (isEmpty || !!session.workspaceMode) && !session.worktreeCwd)
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
        (!remote || !!remoteFeatures?.draft) &&
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
      onBtwCommand={btw.openWith}
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
      usageLimit={session.usageLimit}
      onUsageLimitResume={() => onUsageLimitResume(session.id)}
      onUsageLimitResumeAtReset={(enabled) =>
        onUsageLimitResumeAtReset(session.id, enabled)
      }
      onUsageLimitDismiss={() => onUsageLimitDismiss(session.id)}
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
      className={`${
        agent && !projectBackground ? "" : "chat-pane-background "
      }relative isolate flex h-full min-h-0 min-w-0 flex-1 flex-col`}
      onMouseDown={() => onFocus(session.id)}
    >
      {projectBackground?.effect === "gradient-blur" ||
      (!agent &&
        !projectBackground &&
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
      <div className="flex min-h-0 min-w-0 flex-1">
        <div className="relative flex min-h-0 min-w-0 flex-1 flex-col">
          <div
            ref={transcriptScope}
            className={`@container relative min-h-0 flex-1${
              dockComposer && !agent ? " transcript-composer-fade" : ""
            }`}
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
                onDismiss={() =>
                  onLinkedWorkItemUpdateCardDismiss?.(session.id)
                }
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
                  onDeleteSession
                    ? () => onDeleteSession(session.id)
                    : undefined
                }
              />
            ) : null}
            {remoteSessionLoading ? null : isEmpty ? (
              agent ? (
                <div className="scrollbar-none h-full min-h-0 overflow-y-auto">
                  <MonoHeader agent={agent} greeting />
                </div>
              ) : session.inboxAsk ? (
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
                    blocks={
                      agent && !monoTranscript.viewingOlderPage
                        ? monoPendingTranscriptBlocks(
                            session,
                            monoTranscript.blocks,
                          )
                        : monoTranscript.blocks
                    }
                    historicalBlockIds={monoTranscript.historicalBlockIds}
                    initialTurns={agent ? MONO_PAGE_TURNS : undefined}
                    pageSize={agent ? MONO_PAGE_TURNS : undefined}
                    hasEarlier={monoTranscript.hasEarlier}
                    loadEarlierOnScroll={!!agent}
                    onLoadEarlier={
                      agent ? monoTranscript.loadEarlier : undefined
                    }
                    onReturnToLatest={
                      monoTranscript.viewingOlderPage
                        ? monoTranscript.latest
                        : undefined
                    }
                    busy={!!session.busy && !monoTranscript.viewingOlderPage}
                    visible={visible}
                    cwd={workCwd}
                    agentName={agent?.name}
                    onOpenArtifact={
                      onOpenArtifact
                        ? (id) => onOpenArtifact(session.id, id)
                        : undefined
                    }
                    agentMascot={agent}
                    bottomAligned={!!agent}
                    inlineWork={!!agent}
                    messageDeliveries={agent ? messageDeliveries : undefined}
                    onRetryMessage={
                      agent ? () => onResumeQueue(session.id) : undefined
                    }
                    onShowWork={
                      agent && onShowMonoActivity
                        ? (turnId, blocks) =>
                            onShowMonoActivity(session.id, turnId, blocks)
                        : undefined
                    }
                    activeWorkTurnId={monoActivityTurnId}
                    onShowSessions={
                      agent && onShowMonoSessions
                        ? (turnId, blocks) =>
                            onShowMonoSessions(session.id, turnId, blocks)
                        : undefined
                    }
                    activeSessionsTurnId={monoSessionsTurnId}
                    // A Mono's turn keeps copy, save as note and the time.
                    daySeparators={!!agent}
                    hideTurnMetrics={!!agent}
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
                    onBuildPlan={
                      session.worktreeRemoved ? undefined : buildPlan
                    }
                    planBuildTargets={!remote}
                    onSecondOpinion={
                      !agent &&
                      !session.inboxAsk &&
                      !session.worktreeRemoved &&
                      onSecondOpinion
                        ? (target, turn) =>
                            onSecondOpinion(session.id, target, turn)
                        : undefined
                    }
                    onHandoff={
                      !agent &&
                      !session.inboxAsk &&
                      !session.worktreeRemoved &&
                      onHandoff
                        ? (target, turn) => onHandoff(session.id, target, turn)
                        : undefined
                    }
                    onJumpToBottomChange={jumpVisibility.setVisible}
                    onJumpToBottomReady={onJumpToBottomReady}
                    onRevealReady={onRevealReady}
                    onNavigateReady={onNavigateReady}
                    onScrollerChange={setTranscriptScroller}
                    editingLastTurn={editingLastTurn}
                    onEditLastTurn={
                      editLastTurnSupported && !monoTranscript.viewingOlderPage
                        ? () => {
                            onFocus(session.id);
                            recallLastTurnRef.current?.();
                          }
                        : undefined
                    }
                    latestTurnAccessory={
                      remote ||
                      session.inboxAsk ||
                      session.worktreeRemoved ||
                      monoTranscript.viewingOlderPage ||
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
                    onSearch={monoTranscript.search}
                    side={
                      session.linkedWorkItemUpdateCard &&
                      session.linkedWorkItemUpdateCard.status !== "loading"
                        ? "left"
                        : "right"
                    }
                  />
                ) : null}
                {/* One mark per message would crowd a Mono's endless chat. */}
                {agent ? null : (
                  <PromptOutline
                    blocks={session.blocks}
                    scope={transcriptScope}
                    scroller={transcriptScroller}
                    visible={visible}
                    revealBlock={revealBlock}
                  />
                )}
                <TranscriptJumpToBottom
                  visibility={jumpVisibility}
                  onJump={() => {
                    if (monoTranscript.viewingOlderPage) {
                      monoTranscript.latest();
                      requestAnimationFrame(() => jumpToBottomRef.current?.());
                    } else jumpToBottomRef.current?.();
                  }}
                />
              </>
            )}
          </div>
          {dockComposer ? (
            <div
              ref={composerDockMotion.dockedRef}
              data-session-composer
              inert={btw.open}
              className="mx-auto w-full max-w-4xl shrink-0"
            >
              {agent ? (
                <>
                  <MessageQueue
                    messages={(session.queuedMessages ?? []).filter(
                      (message) => !!message.monoSessionCompletion,
                    )}
                    status={session.queueStatus}
                    sendingId={session.sendingQueuedMessageId}
                    onDelete={(messageId) =>
                      onDeleteQueuedMessage(session.id, messageId)
                    }
                    onResume={() => onResumeQueue(session.id)}
                    variant="messages"
                  />
                  {session.pendingQuestion ? (
                    <QuestionForm
                      prompt={session.pendingQuestion}
                      onReply={replyQuestion}
                      onInteraction={(id) =>
                        onQuestionInteraction?.(session.id, id)
                      }
                    />
                  ) : null}
                  {session.usageLimit ? (
                    <MonoUsageLimitNotice
                      session={session}
                      onModelChange={(harness, model) =>
                        onModelChange(session.id, harness, model)
                      }
                      onAccountChange={
                        onUsageLimitAccountChange
                          ? (accountId) =>
                              onUsageLimitAccountChange(session.id, accountId)
                          : undefined
                      }
                      onResume={() => onUsageLimitResume(session.id)}
                      onResumeAtReset={(enabled) =>
                        onUsageLimitResumeAtReset(session.id, enabled)
                      }
                    />
                  ) : null}
                  <MonoComposer
                    key={session.id}
                    sessionId={session.id}
                    name={agent.name}
                    enabled={visible}
                    quoteRequest={quoteRequest}
                    onQuoteRequestConsumed={acknowledgeQuote}
                    onDraftChange={(text) => {
                      draftRef.current = text;
                    }}
                    focusToken={
                      focused && composerFocused
                        ? composerFocusToken
                        : undefined
                    }
                    onFocus={() => onFocus(session.id)}
                    onSubmit={(text, attachments) =>
                      onSubmit(session.id, text, attachments)
                    }
                  />
                </>
              ) : (
                composer
              )}
            </div>
          ) : null}
          <BtwSheet
            btw={btw}
            cwd={workCwd}
            visible={visible}
            origin={() =>
              composerDockMotion.dockedRef.current?.querySelector<HTMLElement>(
                "[data-composer-box]",
              ) ?? null
            }
            onSaveNote={notesEnabled ? saveNote : undefined}
            onOpenFile={onOpenFile}
            onOpenDiff={onOpenDiff}
          />
        </div>
      </div>
    </div>
  );
});

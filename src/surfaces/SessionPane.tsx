import { ChevronDown, GripVertical, X } from "../chrome/icons";
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
import { Composer } from "../chrome/Composer";
import { DiscussionEmpty } from "../chrome/DiscussionEmpty";
import { SessionReview } from "../chrome/SessionReview";
import { PromptOutline } from "../chrome/PromptOutline";
import {
  canCompactHarnessContext,
  type ApprovalDecision,
  type UserQuestionReply,
} from "../lib/harness";
import { looksLikeProject, type RecentProject } from "../lib/recents";
import {
  sessionDisplayTitle,
  sessionWorkCwd,
  type Attachment,
  type Block,
  type HarnessId,
  type PlanBuildTarget,
  type RuntimeMode,
  type Session,
  type TurnIntent,
} from "../lib/session";
import { AgentTranscript } from "./AgentTranscript";
import { EmptySession } from "./EmptySession";
import { MOD } from "../lib/platform";
import {
  acknowledgeQuoteRequest,
  ADD_TO_CHAT_EVENT,
  type AddToChatRequest,
  type QuoteRequest,
} from "../lib/quoteDraft";
import { createNote, noteTitle } from "../lib/notes";
import { loadNotesEnabled, subscribeNotesEnabled } from "../lib/settings";
import { resolveModel } from "../lib/models";
import { isAstraModel } from "../lib/astraWelcome";
import { AstraWelcome } from "./AstraWelcome";
import { projectKey } from "../lib/paths";
import {
  loadProjectChatBackground,
  projectChatBackgroundRevision,
  subscribeProjectChatBackground,
} from "../lib/projectChatBackground";
import { projectChatBackgroundSrc } from "../lib/chatBackground";
import {
  loadChatBackgroundPath,
  subscribeChatBackgroundPath,
} from "../lib/appearance";
import type { SessionFolderTarget } from "../lib/sessionFolders";

type Props = {
  session: Session;
  reviewUndoLocked?: boolean;
  visible: boolean;
  focused: boolean;
  addToChatTarget?: boolean;
  inSplit: boolean;
  composerFocused: boolean;
  recents: RecentProject[];
  hideProjectPicker?: boolean;
  onFocus: (sessionId: string) => void;
  onClose: (sessionId: string) => void;
  onCwdChange: (sessionId: string, cwd: string) => void;
  onBranchChange: (sessionId: string) => void;
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
    options?: { intent?: TurnIntent },
  ) => void;
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
  onNoteCardDismiss?: (sessionId: string) => void;
  onHandoffCardDismiss?: (sessionId: string) => void;
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
    harness: HarnessId,
    turn: Block[],
    model: string,
  ) => void;
  onHandoff?: (
    sessionId: string,
    harness: HarnessId,
    turn: Block[],
    model: string,
  ) => void;
  onNewTerminal: (sessionId: string) => void;
  onPaneDragStart?: (event: ReactPointerEvent<HTMLElement>) => void;
};

export const SessionPane = memo(function SessionPane({
  session,
  reviewUndoLocked = false,
  visible,
  focused,
  addToChatTarget = focused,
  inSplit,
  composerFocused,
  recents,
  hideProjectPicker,
  onFocus,
  onClose,
  onCwdChange,
  onBranchChange,
  onModelChange,
  onModelSettingsChange,
  onRuntimeModeChange,
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
  onNoteCardDismiss,
  onHandoffCardDismiss,
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
}: Props) {
  const title = sessionDisplayTitle(session.title, session.harness);
  const backgroundRevision = useSyncExternalStore(
    subscribeProjectChatBackground,
    projectChatBackgroundRevision,
    projectChatBackgroundRevision,
  );
  const globalBackgroundPath = useSyncExternalStore(
    subscribeChatBackgroundPath,
    loadChatBackgroundPath,
    loadChatBackgroundPath,
  );
  const projectBackground = loadProjectChatBackground(projectKey(session.cwd));
  const projectBackgroundStyle = projectBackground
    ? ({
        "--chat-background-image": `url(${JSON.stringify(
          projectChatBackgroundSrc(projectBackground.path, backgroundRevision),
        )})`,
        "--chat-background-opacity": String(projectBackground.opacity),
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
  const quoteRequestId = useRef(0);
  const [showJumpToBottom, setShowJumpToBottom] = useState(false);
  const astraWelcomeSequence = useRef(0);
  const [astraWelcomeRun, setAstraWelcomeRun] = useState<number | null>(null);
  const dismissAstraWelcome = useCallback(() => setAstraWelcomeRun(null), []);
  useEffect(() => {
    if (!visible) setAstraWelcomeRun(null);
  }, [visible]);
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
    (text: string) => {
      const sessionTitle = sessionDisplayTitle(session.title, session.harness);
      void createNote({
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
  const isEmpty = session.blocks.length === 0;
  const showDeckProjectPicker = isEmpty && !looksLikeProject(session.cwd);
  const dockComposer = !isEmpty || inSplit || !!session.inboxAsk;
  const draftRef = useRef<string | undefined>(undefined);
  const composer = (
    <Composer
      enabled={visible}
      focused={focused && composerFocused}
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
      hideBranchPicker={!!session.inboxAsk}
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
      onNewTerminal={() => onNewTerminal(session.id)}
      onModelChange={(harness, model) => {
        onModelChange(session.id, harness, model);
        const selected = resolveModel(harness, model);
        // A new key restarts the animation and its cleanup timer on every pick.
        setAstraWelcomeRun(
          isAstraModel(selected) ? ++astraWelcomeSequence.current : null,
        );
      }}
      onModelSettingsChange={(settings) =>
        onModelSettingsChange(session.id, settings)
      }
      onRuntimeModeChange={(mode) => onRuntimeModeChange(session.id, mode)}
      onSubmit={(text, attachments, options) =>
        onSubmit(session.id, text, attachments, options)
      }
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
    />
  );

  return (
    <div
      data-session-drop={session.id}
      data-session-empty={isEmpty}
      data-project-chat-background={!!projectBackground}
      data-project-background-scope={projectBackground?.scope}
      style={projectBackgroundStyle}
      className="chat-pane-background relative isolate flex h-full min-h-0 min-w-0 flex-1 flex-col"
      onMouseDown={() => onFocus(session.id)}
    >
      {astraWelcomeRun !== null && visible ? (
        <AstraWelcome key={astraWelcomeRun} onDone={dismissAstraWelcome} />
      ) : null}
      {inSplit ? (
        <div
          className={`flex h-9 shrink-0 touch-none items-center gap-1.5 border-b border-content/10 px-2 select-none ${
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
      <div ref={transcriptScope} className="@container relative min-h-0 flex-1">
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
              composer={dockComposer ? undefined : composer}
            />
          )
        ) : (
          <>
            <AgentTranscript
              blocks={session.blocks}
              busy={!!session.busy}
              visible={visible}
              cwd={workCwd}
              harness={session.harness}
              model={session.model}
              pendingQuestion={!!session.pendingQuestion}
              onApproval={approve}
              onAddToChat={addSelectionToChat}
              onSaveNote={notesEnabled ? saveNote : undefined}
              onOpenFile={onOpenFile}
              onOpenDiff={onOpenDiff}
              onOpenPlan={openPlan}
              onBuildPlan={buildPlan}
              onSecondOpinion={
                !session.inboxAsk && onSecondOpinion
                  ? (harness, turn, model) =>
                      onSecondOpinion(session.id, harness, turn, model)
                  : undefined
              }
              onHandoff={
                !session.inboxAsk && onHandoff
                  ? (harness, turn, model) =>
                      onHandoff(session.id, harness, turn, model)
                  : undefined
              }
              onJumpToBottomChange={setShowJumpToBottom}
              onJumpToBottomReady={onJumpToBottomReady}
              onRevealReady={onRevealReady}
              latestTurnAccessory={
                session.inboxAsk ? undefined : (
                  <SessionReview
                    sessionId={session.id}
                    cwd={workCwd}
                    enabled={visible}
                    busy={!!session.busy}
                    undoLocked={reviewUndoLocked}
                    onOpenDiff={onOpenDiff}
                  />
                )
              }
            />
            <PromptOutline
              blocks={session.blocks}
              scope={transcriptScope}
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
        <div className="mx-auto w-full max-w-4xl shrink-0">{composer}</div>
      ) : null}
    </div>
  );
});

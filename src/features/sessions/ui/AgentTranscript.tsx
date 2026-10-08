import {
  ArrowUp,
  Chatting,
  Check,
  ChevronRight,
  CircleDashed,
  Copy,
  FilePlusCorner,
  ListBullet,
  Minus,
  Pencil,
  PenLine,
  Bot,
  ChartBreakoutSquare,
  Search,
  Terminal,
  Trash2,
  Wrench,
  X,
} from "../../../shared/ui/icons";
import {
  memo,
  startTransition,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
  type RefObject,
} from "react";
import { flushSync } from "react-dom";
import { AttachmentChip } from "./AttachmentChip";
import { GeneratedImage } from "./GeneratedImage";
import { MonocodeSparkles } from "./MonocodeSparkles";
import { OrchestratorConstellation } from "./OrchestratorConstellation";
import { PlanStepsBurst } from "./PlanStepsBurst";
import { FilePreview } from "../../files/ui/FilePreview";
import { FileTypeIcon } from "../../files/ui/FileTypeIcon";
import { ToolDiffPreview } from "./ToolDiffPreview";
import { PlanPreview } from "./PlanPreview";
import { OrchestrationPreview } from "../../orchestration/ui/OrchestrationPreview";
import { TaskListPreview } from "./TaskListPreview";
import { HandoffButton, SecondOpinionButton } from "./SecondOpinionButton";
import { SecondOpinionCard } from "./SecondOpinionCard";
import { NoteMiniCard } from "../../notes/ui/NoteMiniCard";
import { ArtifactCard } from "../../artifacts/ui/ArtifactCard";
import { artifactCards } from "../../artifacts/artifacts";

import { TerminalSpinner } from "./TerminalSpinner";
import { Popover } from "../../../shared/ui/Popover";
import { ProjectMascot } from "../../projects/ui/ProjectMascot";
import { PixelMascot } from "../../projects/ui/PixelMascot";
import type { MonoLook } from "../../monos/model/mono";
import { monoSpawnedSessions } from "../../monos/model/monoSpawnedSessions";
import type { MessageDelivery } from "../../monos/model/monoMessaging";
import type { ApprovalDecision } from "../../../integrations/harness";
import {
  isHarnessAuthError,
  supportsHarnessLogin,
} from "../../../integrations/harness/core/authSupport";
import {
  isEditTool,
  isReadTool,
  isSearchTool,
  stubFilePreview,
} from "../../../integrations/harness/core/preview";
import { copyMessage } from "../../../platform/tauri/clipboard";
import type { Attachment } from "../model/session";
import { visibleUserPrompt } from "../../orchestration/model/orchestration";
import { playCue } from "../../settings/model/sounds";
import { legacyTaskListFromText } from "../model/taskList";
import { resolveModel } from "../model/models";
import { harnessForTurn } from "../model/secondOpinion";
import { TranscriptTurnCache } from "../model/transcriptTurnCache";
import { Shimmer } from "../../../shared/ui/Shimmer";
import {
  hasPendingApproval,
  HARNESS_TITLE,
  type AgentStep,
  type Block,
  type HarnessId,
  type InterjectionMeta,
  type ModelTarget,
  type PlanBuildTarget,
  type ToolPreview,
  type TurnMetrics,
} from "../model/session";
import { HarnessIcon } from "./HarnessIcon";
import {
  innerScrollerTakes,
  useLockOverscroll,
} from "../../../shared/hooks/useLockOverscroll";
import { useTranscriptLayout } from "../hooks/useTranscriptLayout";
import { useTranscriptAnchor } from "../hooks/useTranscriptAnchor";
import { useBottomChatMotion } from "../hooks/useBottomChatMotion";
import { useTranscriptSelection } from "../hooks/useTranscriptSelection";
import type { TranscriptLayout } from "../../settings/model/appearance";
import { AgentMarkdown } from "./AgentMarkdown";
import { MonoWorkTicker } from "./MonoWorkTicker";
import { monoWorkStatus } from "../model/monoWorkStatus";
import { isEmojiOnlyMessage } from "../model/emojiMessage";
import { attachmentPreviewSrc } from "../model/attachments";
import { TranscriptSelectionMenu } from "./TranscriptSelectionMenu";
import { parseUserMessageLink } from "../model/linkPreview";
import { UserLinkPreview } from "./UserLinkPreview";
import {
  activityPhaseTitle,
  activityStillRunning,
  buildActivityPhases,
  firstFoldableIndex,
  foldableWork,
  foldedBlocks,
  initialThinkingIndex,
  isFailedStatus,
  isIncompleteTool,
  isSubagentBlock,
  isThinkingBlock,
  lastActivityIndex,
  isProseBlock,
  needsApproval,
  nestedScrollAbsorbsWheel,
  proseSummary,
  resolveToolCallDisplay,
  subagentBrief,
  subagentModelName,
  subagentName,
  subagentReport,
  toolCallLabel,
  toolCallState,
  turnCopyText,
  workKind,
  workSummaryLine,
  monoTurnLatestStart,
  monoTurnRuns,
  opensNewStretch,
  type ActivityPhase,
  type ActivityPhaseKind,
  type ToolCallState,
  type TurnItem,
} from "../model/transcriptActivity";
import { lastUserTurnBlock } from "../model/editLastTurn";
import {
  monoCodeToolCall,
  monoCodeWorkSummary,
  type MonoCodeToolCall,
} from "../model/monocodeToolCall";
import {
  isOperatorUserTurn,
  operatorUserPrompt,
} from "../model/operatorCommand";
import {
  clearTranscriptHighlights,
  paintTranscriptHighlights,
  transcriptMutationNeedsRepaint,
  transcriptWordRanges,
} from "../model/transcriptHighlights";

const NEAR_BOTTOM_PX = 16;
const WHEEL_HOLD_MS = 150;
/*
 * Tool calls often land in a burst. Each arrival waits for the one before it
 * to finish its whole entrance — rail, branch, row — before starting its own.
 * The first few play at STEP_ENTRANCE_MS; a queue running past
 * STEP_QUEUE_CALM_MS plays the rest faster, down to STEP_ENTRANCE_MIN_MS by
 * STEP_QUEUE_MS, so a long burst still catches up.
 */
const STEP_ENTRANCE_MS = 480;
const STEP_ENTRANCE_MIN_MS = 160;
const STEP_QUEUE_CALM_MS = 960;
const STEP_QUEUE_MS = 2000;
const INITIAL_TURNS = 20;
/**
 * Turns built before a transcript first paints. Every turn in the initial
 * window costs markdown work on open, so paint the latest few (more if they
 * leave the viewport short) and build the rest of the window after.
 */
const FIRST_PAINT_TURNS = 3;
const TURN_PAGE_SIZE = 20;

type Props = {
  blocks: Block[];
  /** Archived messages render immediately, even when first loaded in this visit. */
  historicalBlockIds?: ReadonlySet<string>;
  initialTurns?: number;
  pageSize?: number;
  hasEarlier?: boolean;
  loadEarlierOnScroll?: boolean;
  onLoadEarlier?: (beforePrepend: () => void) => Promise<void>;
  onReturnToLatest?: () => void;
  busy?: boolean;
  cwd?: string;
  /** Who the turns are by, in place of the model: a Mono's name. */
  agentName?: string;
  /** A Mono's mascot and color, in place of the provider's mark. */
  agentMascot?: Pick<MonoLook, "mascot" | "color">;
  /** Keeps short conversations at the bottom and skips prompt-to-top anchoring. */
  bottomAligned?: boolean;
  /** Keeps process in the activity trail, with live progress in the header. */
  inlineWork?: boolean;
  messageDeliveries?: ReadonlyMap<string, MessageDelivery>;
  onRetryMessage?: (blockId: string) => void;
  /** Inspect a Mono turn in the activity sidebar. */
  onShowWork?: (turnId: string, blocks: Block[]) => void;
  activeWorkTurnId?: string;
  onShowSessions?: (turnId: string, blocks: Block[]) => void;
  activeSessionsTurnId?: string;
  /** Marks when each message was sent, for one conversation kept over days. */
  daySeparators?: boolean;
  /** Leaves token and speed details out of each turn's footer. */
  hideTurnMetrics?: boolean;
  harness?: HarnessId;
  model?: string;
  modelSettings?: Record<string, string>;
  pendingQuestion?: boolean;
  /** Work the agent left running when it yielded; the turn waits on it. */
  backgroundTasks?: string[];
  onApproval?: (requestId: number, decision: ApprovalDecision) => void;
  onAddToChat?: (text: string) => void;
  onSaveNote?: (text: string) => void | Promise<void>;
  onSendDraft?: (block: Block) => boolean | void;
  onRemoveDraft?: (block: Block) => boolean | void;
  onSaveSelectionNote?: (text: string) => void | Promise<void>;
  onOpenFile?: (path: string) => void;
  onOpenArtifact?: (id: string) => void;
  onOpenDiff?: (path: string) => void;
  onOpenPlan?: (blockId: string) => void;
  onBuildPlan?: (blockId: string, target?: PlanBuildTarget) => void;
  planBuildTargets?: boolean;
  onSecondOpinion?: (target: ModelTarget, turn: Block[]) => void;
  onHandoff?: (target: ModelTarget, turn: Block[]) => void;
  onEditLastTurn?: () => void;
  editingLastTurn?: boolean;
  onJumpToBottomChange?: (show: boolean) => void;

  onJumpToBottomReady?: (jump: () => void) => void;
  /** Passes a function that renders the turn that holds a block. The render completes before the function returns. */
  onRevealReady?: (reveal: (blockId: string) => boolean) => void;
  onNavigateReady?: (
    navigate: (blockId: string | null, query?: string) => boolean,
  ) => void;
  /** Session-level output shown after the latest reply and before its action row. */
  latestTurnAccessory?: ReactNode;
  /** False while another tab is in front; local transcript state is retained. */
  visible?: boolean;
  /** Kept mounted after its pane closed. Showing it again counts as a new visit. */
  parked?: boolean;
  onScrollerChange?: (el: HTMLDivElement | null) => void;
  /** A worker's transcript: show the orchestrator's turns instead of hiding them. */
  managed?: boolean;
};

function AgentTranscriptComponent({
  blocks: sourceBlocks,
  historicalBlockIds,
  initialTurns = INITIAL_TURNS,
  pageSize = TURN_PAGE_SIZE,
  hasEarlier = false,
  loadEarlierOnScroll = false,
  onLoadEarlier,
  onReturnToLatest,
  busy,
  cwd,
  agentName,
  agentMascot,
  bottomAligned = false,
  inlineWork = false,
  messageDeliveries,
  onRetryMessage,
  onShowWork,
  activeWorkTurnId,
  onShowSessions,
  activeSessionsTurnId,
  daySeparators = false,
  hideTurnMetrics = false,
  harness,
  model,
  modelSettings,
  pendingQuestion = false,
  backgroundTasks,
  onApproval,
  onAddToChat,
  onSaveNote,
  onSendDraft,
  onRemoveDraft,
  onSaveSelectionNote,
  onOpenFile,
  onOpenArtifact,
  onOpenDiff,
  onOpenPlan,
  onBuildPlan,
  planBuildTargets = true,
  onSecondOpinion,
  onHandoff,
  onEditLastTurn,
  editingLastTurn = false,
  onJumpToBottomChange,
  onJumpToBottomReady,
  onRevealReady,
  onNavigateReady,
  latestTurnAccessory,

  visible = true,
  parked = false,
  onScrollerChange,
  managed = false,
}: Props) {
  const blocks = useMemo(() => {
    if (!harness || !supportsHarnessLogin(harness)) return sourceBlocks;
    const visibleBlocks = sourceBlocks.filter(
      (block) =>
        !(
          block.role === "system" &&
          block.notice === "error" &&
          isHarnessAuthError(block.text)
        ),
    );
    return visibleBlocks.length === sourceBlocks.length
      ? sourceBlocks
      : visibleBlocks;
  }, [harness, sourceBlocks]);
  const editableUserBlockId = useMemo(
    () => lastUserTurnBlock(blocks)?.id,
    [blocks],
  );
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const scroller = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);
  const refreshChatMotion = useRef<(() => void) | null>(null);
  const showJumpRef = useRef(false);
  const distanceFromBottom = useRef(0);
  const lastScrollTop = useRef(0);
  const wheelHold = useRef(0);
  const prependHeight = useRef<number | null>(null);
  const prependAnchor = useRef<{ element: HTMLElement; top: number } | null>(
    null,
  );
  const wasVisible = useRef(false);
  const [scrollerEl, setScrollerEl] = useState<HTMLDivElement | null>(null);
  const seenBlocks = useRef<Set<string> | null>(null);
  if (!seenBlocks.current) {
    seenBlocks.current = new Set(
      blocks.filter((block) => block.text).map((block) => block.id),
    );
  }
  useLayoutEffect(() => {
    for (const block of blocks) {
      if (block.text) seenBlocks.current!.add(block.id);
    }
  }, [blocks]);
  const [visibleTurnCount, setVisibleTurnCount] = useState(
    bottomAligned ? initialTurns : Math.min(FIRST_PAINT_TURNS, initialTurns),
  );
  const [loadingEarlier, setLoadingEarlier] = useState(false);
  const [loadEarlierError, setLoadEarlierError] = useState(false);
  const loadEarlierPending = useRef(false);
  // Turns whose folded work the reader has opened, by turn id.
  const [openWork, setOpenWork] = useState<Record<string, boolean>>({});
  const [searchCurrent, setSearchCurrent] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const highlightOwner = useRef(Symbol("transcript-search"));
  const toggleWork = useCallback((turnId: string, currentlyOpen: boolean) => {
    setOpenWork((open) => ({ ...open, [turnId]: !currentlyOpen }));
  }, []);
  // Stretch the last turn after a send while this tab stays open. Closing
  // the tab is a new visit: the remount uses the true transcript height so
  // the latest reply sits near the composer instead of a hole of empty space.
  const [anchorTurn, setAnchorTurn] = useState(!!busy);
  // Parking detaches the scroller, which drops its scroll offset.
  const restoreScroll = useRef(false);
  const wasParked = useRef(parked);
  if (wasParked.current !== parked) {
    wasParked.current = parked;
    if (parked) {
      restoreScroll.current = true;
      setSearchCurrent(null);
      setSearchQuery("");
    } else if (anchorTurn !== !!busy) {
      setAnchorTurn(!!busy);
    }
  }
  const { selection, dismissSelection } = useTranscriptSelection(
    scrollerEl,
    onAddToChat !== undefined || onSaveSelectionNote !== undefined,
  );
  const transcriptLayout = useTranscriptLayout();
  const promptAnchor = useTranscriptAnchor() && !bottomAligned;
  const lastUserId = lastUserBlockId(blocks, managed);
  const seenUserId = useRef(lastUserId);
  if (lastUserId !== seenUserId.current) {
    seenUserId.current = lastUserId;
    if (lastUserId && !anchorTurn) setAnchorTurn(true);
  }
  const currentModelName = harness
    ? resolveModel(harness, model).name
    : undefined;
  const waitingForApproval = hasPendingApproval(blocks) || pendingQuestion;
  const preparingHandoff = blocks.some(
    (block) =>
      block.role === "handoff" && block.handoff?.status === "preparing",
  );

  const setShowJump = useCallback(
    (show: boolean) => {
      if (showJumpRef.current === show) return;
      showJumpRef.current = show;
      onJumpToBottomChange?.(show);
    },
    [onJumpToBottomChange],
  );

  const syncPinned = useCallback(
    (el: HTMLElement) => {
      // Layout and our own pins also queue scroll events. Those events must
      // not stop a Mono following the end before its layout has settled.
      // Upward wheel, touch, keyboard and scrollbar input release the pin.
      if (bottomAligned && stickToBottom.current) {
        lastScrollTop.current = el.scrollTop;
        distanceFromBottom.current = 0;
        setShowJump(false);
        return;
      }
      const wasFollowing = stickToBottom.current;
      const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
      // Content growth changes the distance without moving the reader. A
      // queued event from a previous pin must not unpin (or re-pin) the view.
      // A taller viewport or shorter transcript can also clamp the previous
      // offset to the new bottom; that is a layout adjustment, not a scroll up.
      stickToBottom.current = followsAfterScroll(
        el,
        lastScrollTop.current,
        stickToBottom.current,
      );
      lastScrollTop.current = el.scrollTop;
      distanceFromBottom.current = distance;
      if (stickToBottom.current && !wasFollowing) refreshChatMotion.current?.();
      setShowJump(!stickToBottom.current && el.scrollHeight > el.clientHeight);
    },
    [bottomAligned, setShowJump],
  );

  const rememberScroll = useCallback((el: HTMLElement) => {
    lastScrollTop.current = el.scrollTop;
    distanceFromBottom.current =
      el.scrollHeight - el.scrollTop - el.clientHeight;
  }, []);

  const pinTranscript = useCallback(
    (el: HTMLElement | null) => {
      if (!el) return;
      pinToBottom(el);
      rememberScroll(el);
    },
    [rememberScroll],
  );

  const followTranscript = useCallback(
    (el: HTMLElement | null) => {
      if (!el) return;
      // The browser can apply a manual scroll before dispatching its event.
      // Reconcile that offset before a streaming commit or observer pins it.
      syncPinned(el);
      // A gesture whose direction is not known yet may already be scrolling
      // off the main thread. Pinning now would snap it back to the end.
      if (stickToBottom.current && performance.now() >= wheelHold.current)
        pinTranscript(el);
    },
    [pinTranscript, syncPinned],
  );

  const jumpToBottom = useCallback(() => {
    stickToBottom.current = true;
    distanceFromBottom.current = 0;
    setShowJump(false);
    const el = scroller.current;
    syncTranscriptViewport(el);
    pinTranscript(el);
    refreshChatMotion.current?.();
  }, [pinTranscript, setShowJump]);

  const setScroller = useCallback(
    (el: HTMLDivElement | null) => {
      scroller.current = el;
      setScrollerEl(el);
      // The Mono already has overscroll-none. Prefer native scrolling where
      // supported so its first wheel event need not wait on a blocking listener.
      const nativeOverscroll =
        bottomAligned && CSS.supports?.("overscroll-behavior", "none");
      lockOverscroll(nativeOverscroll ? null : el);
    },
    [bottomAligned, lockOverscroll],
  );

  useEffect(() => {
    onJumpToBottomReady?.(jumpToBottom);
  }, [jumpToBottom, onJumpToBottomReady]);

  // A pooled transcript outlives its pane; tell each new owner where it stands.
  useEffect(() => {
    onJumpToBottomChange?.(showJumpRef.current);
  }, [onJumpToBottomChange]);

  useLayoutEffect(() => {
    onScrollerChange?.(scrollerEl);
    return () => onScrollerChange?.(null);
  }, [onScrollerChange, scrollerEl]);

  useEffect(() => {
    if (!visible || !scrollerEl) return;
    const onScroll = () => {
      if (scrollerEl.isConnected && scrollerEl.clientHeight > 0)
        syncPinned(scrollerEl);
    };
    let release: ReturnType<typeof setTimeout> | undefined;
    const onWheel = (e: WheelEvent) => {
      if (innerScrollerTakes(scrollerEl, e)) return;
      if (e.deltaY < 0) {
        stickToBottom.current = false;
        setShowJump(true);
      } else if (e.deltaY === 0) {
        // A trackpad gesture can open with an event that carries no
        // direction, and the rest of it may reach us after the scroll has
        // moved. Hold the pin until its upward events can release it.
        wheelHold.current = performance.now() + WHEEL_HOLD_MS;
        clearTimeout(release);
        release = setTimeout(() => {
          if (scrollerEl.isConnected) followTranscript(scrollerEl);
        }, WHEEL_HOLD_MS);
      }
    };
    const onPointerDown = (event: PointerEvent) => {
      if (bottomAligned && event.target === scrollerEl) {
        stickToBottom.current = false;
      }
    };
    scrollerEl.addEventListener("scroll", onScroll, { passive: true });
    scrollerEl.addEventListener("wheel", onWheel, { passive: true });
    scrollerEl.addEventListener("pointerdown", onPointerDown, {
      passive: true,
    });
    return () => {
      clearTimeout(release);
      scrollerEl.removeEventListener("scroll", onScroll);
      scrollerEl.removeEventListener("wheel", onWheel);
      scrollerEl.removeEventListener("pointerdown", onPointerDown);
    };
  }, [
    bottomAligned,
    scrollerEl,
    followTranscript,
    setShowJump,
    syncPinned,
    visible,
  ]);

  useLayoutEffect(() => {
    stickToBottom.current = true;
    setShowJump(false);
    const el = scroller.current;
    syncTranscriptViewport(el);
    pinTranscript(el);
  }, [lastUserId, pinTranscript, setShowJump]);

  // In the chat layout a sent prompt rises from the upper screen into its
  // anchored spot at the top. On mount this only plays for a session's first
  // send.
  const introducePrompt = useRef({ chat: false, anchor: false, visible });
  introducePrompt.current = {
    chat: transcriptLayout === "chat",
    anchor: promptAnchor && anchorTurn,
    visible,
  };
  const introducedPromptMount = useRef(false);
  useLayoutEffect(() => {
    const mounting = !introducedPromptMount.current;
    introducedPromptMount.current = true;
    const { chat, anchor, visible } = introducePrompt.current;
    if (!lastUserId || !chat || !anchor || !visible) return;
    if (mounting && !(busy && userTurnCount(blocks, managed) === 1)) return;
    return riseIntoAnchor(scroller.current, lastUserId);
    // Only a new prompt starts the motion; later renders must not replay it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lastUserId]);

  useLayoutEffect(() => {
    const opened = visible && !wasVisible.current;
    wasVisible.current = visible;
    if (!opened) return;
    const el = scroller.current;
    if (!el) return;
    syncTranscriptViewport(el);
    const restore = restoreScroll.current;
    restoreScroll.current = false;
    // Previously opened tabs normally retain their scroll position. Only pin
    // when the scroller looks empty after being hidden with `display: none`.
    if (el.scrollHeight <= el.clientHeight + NEAR_BOTTOM_PX) {
      stickToBottom.current = true;
      setShowJump(false);
      pinTranscript(el);
    } else if (restore && stickToBottom.current) {
      // Reattaching reset the offset to the top. Pin before any follow reads
      // that reset as the reader scrolling up and lets go of the end.
      pinTranscript(el);
    } else if (restore) {
      el.scrollTop = Math.max(
        0,
        el.scrollHeight - el.clientHeight - distanceFromBottom.current,
      );
      rememberScroll(el);
    }
  }, [visible, pinTranscript, rememberScroll, setShowJump]);

  useLayoutEffect(() => {
    if (!visible || !stickToBottom.current) return;
    const el = scroller.current;
    syncTranscriptViewport(el);
    followTranscript(el);
  }, [blocks, busy, followTranscript, visible]);

  useLayoutEffect(() => {
    const el = scrollerEl;
    const inner = el?.firstElementChild;
    if (!visible || !el || !inner) return;
    const onResize = () => {
      // A parked transcript's scroller is detached and measures zero.
      if (!el.isConnected) return;
      syncTranscriptViewport(el);
      followTranscript(el);
      if (bottomAligned && !stickToBottom.current)
        setShowJump(!isNearBottom(el, 1));
    };
    const observer = new ResizeObserver(onResize);
    observer.observe(inner);
    observer.observe(el);
    onResize();
    return () => observer.disconnect();
  }, [bottomAligned, scrollerEl, followTranscript, setShowJump, visible]);

  useTurnScrollAnchor(scrollerEl, visible, stickToBottom, rememberScroll);

  const [turnCache] = useState(() => new TranscriptTurnCache());
  const undeliveredMessageIds = useMemo(
    () => new Set(messageDeliveries?.keys()),
    [messageDeliveries],
  );
  const turns = turnCache.group(blocks, managed, inlineWork);
  // A scheduled update can land while the chat's own turn is still running.
  const activeTurnIndex = turns.reduce(
    (latest, turn, index) =>
      turn[0].monoHabit || turn[0].role === "handoff" ? latest : index,
    -1,
  );
  const firstVisibleTurn = Math.max(0, turns.length - visibleTurnCount);
  const visibleTurns = turns.slice(firstVisibleTurn);
  const turnsRef = useRef(turns);
  turnsRef.current = turns;
  const visibleTurnCountRef = useRef(visibleTurnCount);
  visibleTurnCountRef.current = visibleTurnCount;

  useLayoutEffect(() => {
    const previousHeight = prependHeight.current;
    const el = scroller.current;
    if (!el) return;
    if (previousHeight == null) {
      // The opening window grows above the screen. Settle the offset in this
      // commit: a scroll event queued by an earlier pin would otherwise read
      // the taller transcript first and unpin it partway up. The insert can
      // also nudge the offset itself, so do not read that as the reader
      // scrolling; a wheel or touch has already released the pin.
      if (stickToBottom.current) {
        syncTranscriptViewport(el);
        pinTranscript(el);
      } else {
        el.scrollTop =
          el.scrollHeight - el.clientHeight - distanceFromBottom.current;
        rememberScroll(el);
      }
      return;
    }
    prependHeight.current = null;
    const anchor = prependAnchor.current;
    prependAnchor.current = null;
    const shift = anchor?.element.isConnected
      ? anchor.element.getBoundingClientRect().top -
        el.getBoundingClientRect().top +
        el.scrollTop -
        anchor.top
      : el.scrollHeight - previousHeight;
    el.scrollTop += shift;
    rememberScroll(el);
  }, [visibleTurnCount, pinTranscript, rememberScroll]);

  // Short turns can leave the first paint with empty space above them, and
  // the rest of the window arriving later would then push everything down.
  // Top up before painting until the viewport is covered.
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el || el.clientHeight === 0) return;
    if (visibleTurnCount >= Math.min(initialTurns, turns.length)) return;
    if (el.scrollHeight > el.clientHeight) return;
    setVisibleTurnCount((count) =>
      Math.min(initialTurns, count + FIRST_PAINT_TURNS),
    );
  }, [visibleTurnCount, turns.length, initialTurns]);

  useEffect(() => {
    if (bottomAligned) return;
    // Interruptible, so switching away before it finishes costs nothing.
    startTransition(() =>
      setVisibleTurnCount((count) => Math.max(count, initialTurns)),
    );
  }, [bottomAligned, initialTurns]);

  const prepareToPrepend = useCallback(() => {
    const el = scroller.current;
    if (el) {
      prependHeight.current = el.scrollHeight;
      prependAnchor.current = null;
      if (bottomAligned) {
        const viewportTop = el.getBoundingClientRect().top;
        for (const turn of el.querySelectorAll<HTMLElement>(
          ".transcript-turn",
        )) {
          const rect = turn.getBoundingClientRect();
          if (rect.bottom <= viewportTop) continue;
          // Content coordinates keep subsequent native scrolling independent
          // of the height added above this exact message.
          prependAnchor.current = {
            element: turn,
            top: rect.top - viewportTop + el.scrollTop,
          };
          break;
        }
      }
    }
    stickToBottom.current = false;
  }, [bottomAligned]);

  const loadEarlier = useCallback(async () => {
    if (loadEarlierPending.current) return;
    if (firstVisibleTurn === 0 && (!onLoadEarlier || !hasEarlier)) return;
    setLoadEarlierError(false);
    if (firstVisibleTurn > 0) {
      prepareToPrepend();
      setVisibleTurnCount((count) => Math.min(turns.length, count + pageSize));
      return;
    }
    if (!onLoadEarlier || !hasEarlier) return;
    loadEarlierPending.current = true;
    setLoadingEarlier(true);
    try {
      // Snapshot at the arrival of the page, after any scrolling or streamed
      // output during the request, and before the older DOM is inserted.
      let prepared = false;
      await onLoadEarlier(() => {
        prepareToPrepend();
        prepared = true;
      });
      if (prepared) setVisibleTurnCount((count) => count + pageSize);
    } catch {
      prependHeight.current = null;
      prependAnchor.current = null;
      setLoadEarlierError(true);
    } finally {
      loadEarlierPending.current = false;
      setLoadingEarlier(false);
    }
  }, [
    firstVisibleTurn,
    hasEarlier,
    onLoadEarlier,
    pageSize,
    prepareToPrepend,
    turns.length,
  ]);

  useEffect(() => {
    if (!loadEarlierOnScroll || !visible || !scrollerEl) return;
    let previousTop = scrollerEl.scrollTop;
    const onScroll = () => {
      const top = scrollerEl.scrollTop;
      const scrollingUp = top < previousTop;
      previousTop = top;
      if (scrollingUp && top <= 160 && !stickToBottom.current)
        void loadEarlier();
    };
    // A short page may fit the viewport, so scrolling upward can reach the
    // boundary without changing scrollTop or firing a scroll event.
    const onWheel = (event: WheelEvent) => {
      if (event.deltaY < 0 && scrollerEl.scrollTop <= 160) void loadEarlier();
    };
    scrollerEl.addEventListener("scroll", onScroll, { passive: true });
    scrollerEl.addEventListener("wheel", onWheel, { passive: true });
    return () => {
      scrollerEl.removeEventListener("scroll", onScroll);
      scrollerEl.removeEventListener("wheel", onWheel);
    };
  }, [loadEarlier, loadEarlierOnScroll, scrollerEl, visible]);

  const revealBlock = useCallback(
    (blockId: string): boolean => {
      const all = turnsRef.current;
      const index = all.findIndex((turn) =>
        turn.some((block) => block.id === blockId),
      );
      if (index < 0) return false;
      const needed = all.length - index;
      if (needed <= visibleTurnCountRef.current) return true;
      prepareToPrepend();
      // Synchronous. The caller finds the turn in the DOM after this call.
      flushSync(() => setVisibleTurnCount(needed));
      return true;
    },
    [prepareToPrepend],
  );

  useEffect(() => {
    onRevealReady?.(revealBlock);
  }, [revealBlock, onRevealReady]);

  const navigateToBlock = useCallback(
    (blockId: string | null, query = ""): boolean => {
      if (!blockId) {
        setSearchCurrent(null);
        setSearchQuery("");
        return true;
      }
      const turn = turnsRef.current.find((item) =>
        item.some((block) => block.id === blockId),
      );
      if (!turn || !revealBlock(blockId)) return false;
      const turnId = turn[0].id;
      // A result inside folded work needs its row rendered before measuring it.
      flushSync(() => {
        setOpenWork((current) =>
          current[turnId] ? current : { ...current, [turnId]: true },
        );
        setSearchCurrent(blockId);
        setSearchQuery(query);
      });
      const el = scroller.current;
      if (!el) return false;
      el.dispatchEvent(new WheelEvent("wheel", { deltaY: -1 }));
      const align = () => {
        const target =
          el.querySelector<HTMLElement>(
            '[data-transcript-search-current="true"]',
          ) ??
          el.querySelector<HTMLElement>(
            `[data-transcript-turn="${CSS.escape(turnId)}"]`,
          );
        if (!target) return;
        const wordRect = query
          ? transcriptWordRanges(el, query).current?.getBoundingClientRect?.()
          : null;
        const targetTop =
          wordRect && wordRect.height > 0
            ? wordRect.top
            : target.getBoundingClientRect().top;
        const delta = targetTop - el.getBoundingClientRect().top - 42;
        if (Math.abs(delta) > 2) {
          el.scrollTop += delta;
          rememberScroll(el);
        }
      };
      align();
      requestAnimationFrame(align);
      return true;
    },
    [revealBlock, rememberScroll],
  );

  useEffect(() => {
    onNavigateReady?.(navigateToBlock);
  }, [navigateToBlock, onNavigateReady]);

  useEffect(() => {
    const el = scroller.current;
    const owner = highlightOwner.current;
    if (!el || !visible || !searchQuery) {
      clearTranscriptHighlights(owner);
      return;
    }
    let frame = 0;
    let pending: MutationRecord[] = [];
    const paint = () => {
      const { matches, current } = transcriptWordRanges(el, searchQuery);
      paintTranscriptHighlights(owner, matches, current);
    };
    const observer = new MutationObserver((records) => {
      for (const record of records) pending.push(record);
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        const changed = pending;
        pending = [];
        if (transcriptMutationNeedsRepaint(changed, searchQuery)) paint();
      });
    });
    observer.observe(el, {
      childList: true,
      characterData: true,
      subtree: true,
    });
    paint();
    return () => {
      observer.disconnect();
      if (frame) cancelAnimationFrame(frame);
      clearTranscriptHighlights(owner);
    };
  }, [visible, searchQuery, searchCurrent, visibleTurnCount, openWork]);

  refreshChatMotion.current = useBottomChatMotion(
    scrollerEl,
    visible && bottomAligned,
    stickToBottom,
    blocks,
    !!busy && userTurnCount(blocks, managed) === 1,
    !!busy,
    historicalBlockIds,
  );

  return (
    <div
      ref={setScroller}
      className={`agent-transcript h-full overflow-y-auto overscroll-none [overflow-anchor:none] font-mono text-[13px] leading-5${bottomAligned ? " mono-transcript-fades scrollbar-none" : ""}`}
    >
      <TranscriptContent bottomAligned={bottomAligned}>
        {onReturnToLatest ? (
          <div className="flex justify-center px-4 py-2">
            <button
              type="button"
              className="text-content/60 hover:text-content"
              onClick={() => {
                stickToBottom.current = true;
                prependHeight.current = null;
                setVisibleTurnCount(initialTurns);
                setSearchCurrent(null);
                setSearchQuery("");
                onReturnToLatest();
              }}
            >
              Back to latest messages
            </button>
          </div>
        ) : null}
        {loadEarlierOnScroll && (loadingEarlier || loadEarlierError) ? (
          <div
            role="status"
            className="pointer-events-none sticky top-0 z-10 h-0 text-center font-sans text-[12px] text-content/60"
          >
            <span className="inline-block rounded-md bg-background-base px-2.5 py-1.5">
              {loadingEarlier
                ? "Loading earlier messages…"
                : "Couldn't load earlier messages. Scroll up to retry."}
            </span>
          </div>
        ) : null}
        {!loadEarlierOnScroll && (firstVisibleTurn > 0 || hasEarlier) ? (
          <div className="flex justify-center px-4 py-3">
            <button
              type="button"
              className="rounded-md bg-content/8 px-2.5 py-1.5 font-sans text-[12px] text-content/60 hover:bg-content/12 hover:text-content"
              onClick={() => void loadEarlier()}
              disabled={loadingEarlier}
            >
              {loadingEarlier
                ? "Loading earlier messages…"
                : loadEarlierError
                  ? "Couldn't load messages. Try again"
                  : "Load earlier messages"}
            </button>
          </div>
        ) : null}
        {visibleTurns.map((turn, turnIndex) => {
          const isLastTurn = firstVisibleTurn + turnIndex === turns.length - 1;
          // A Mono's own follow-up replies join the turn they follow; each
          // run keeps its prompt, but the turn reads as one message.
          const runs = inlineWork ? monoTurnRuns(turn) : [turn];
          const userBlock = inlineWork
            ? monoTurnUserBlock(runs[0], messageDeliveries)
            : turnUserBlock(turn, managed);
          const lastRun =
            runs.length > 1
              ? monoTurnUserBlock(runs[runs.length - 1], messageDeliveries)
              : userBlock;
          const habit = turn[0].monoHabit;
          // Older saved reports may have lost their habit tag. A Mono's
          // standalone reply still needs its identity and response actions.
          const standaloneReply =
            !userBlock && !!(agentName || habit) && turn.some(isProseBlock);
          const durationMs =
            runs.length > 1
              ? sumDurations(
                  runs.map(
                    (run) =>
                      monoTurnUserBlock(run, messageDeliveries)?.durationMs,
                  ),
                )
              : userBlock?.durationMs;
          const settled = !(
            busy &&
            !standaloneReply &&
            firstVisibleTurn + turnIndex === activeTurnIndex
          );
          const proposals = turn.filter((block) => block.orchestration);
          const artifacts = artifactCards(turn);
          // Proposals are turn results, like the changes card. Keep them out
          // of the live work and append them after all of the lead's output.
          const items = turnCache.turnItems(turn, settled, {
            managed,
            inlineWork,
            undeliveredMessageIds,
          });
          // Earlier activity groups have already been followed by prose or
          // more work. Only the last one can still be the live group.
          const foldedAt = lastActivityIndex(items);
          const initialThinkingAt = initialThinkingIndex(items);
          const startedAt =
            userBlock?.startedAt ?? habit?.at ?? turn[0].startedAt;
          const previousTurn = turns[firstVisibleTurn + turnIndex - 1];
          const previousAt = !previousTurn
            ? undefined
            : inlineWork
              ? monoTurnLatestStart(previousTurn)
              : (turnUserBlock(previousTurn, managed)?.startedAt ??
                previousTurn[0].monoHabit?.at ??
                previousTurn[0].startedAt);
          const stampAt =
            daySeparators &&
            startedAt != null &&
            opensNewStretch(startedAt, previousAt)
              ? startedAt
              : undefined;
          // The agent starting its answer is the end of the work: fold the
          // groups then, not when the turn finally settles, so the collapse
          // never lands under the text you have already started reading.
          const answering =
            foldedAt >= 0 &&
            items
              .slice(foldedAt + 1)
              .some(
                (item) => item.type === "block" && isProseBlock(item.block),
              );
          const workStillRunning = activityStillRunning(turn);
          // New turns carry immutable model provenance. Legacy turns do not,
          // so omit their model instead of rewriting history from the picker.
          const turnModel = userBlock?.turnModel;
          const turnHarness = harness
            ? (turnModel?.harness ?? harnessForTurn(blocks, turn, harness))
            : undefined;
          // Work the turn has already answered for folds away behind one line,
          // leaving the prompt and the answer to it.
          const turnId = turn[0].id;
          const spawnedSessions = inlineWork ? monoSpawnedSessions(turn) : [];
          const fold = inlineWork ? undefined : foldableWork(items);
          const folded = fold ? foldedBlocks(items, fold) : [];
          const summarizedWork = inlineWork
            ? items.flatMap((item) =>
                item.type === "block" ? [] : item.blocks,
              )
            : folded;
          const workOpen = openWork[turnId] ?? false;
          // The fold line is the turn's status line from the first token to
          // the last: the mark, and the clock beside it. It never moves, so a
          // turn settling does not shuffle the layout around the answer.
          const live = visible && !settled && !preparingHandoff;
          // A Mono's turns are its own, whichever model ran them.
          const turnModelName =
            agentName ??
            turnModel?.name ??
            (live ? currentModelName : undefined);
          // The fold line speaks for the main agent only. A delegated run has
          // its own row, which says who is working and how it went, so saying
          // it again here would be two lines telling the same story.
          const foldTitle: ReactNode =
            habit || standaloneReply ? (
              (agentName ?? habit?.name)
            ) : live ? (
              <LiveFoldTitle
                startedAt={startedAt}
                paused={waitingForApproval}
                waitingLabel={
                  managed && waitingForApproval
                    ? "Waiting for orchestrator"
                    : pendingQuestion
                      ? "Waiting for answers"
                      : undefined
                }
                background={backgroundTasks}
                modelName={turnModelName}
              />
            ) : agentMascot && agentName ? (
              // A Mono signs its settled turns with just its mascot and name.
              <span className="font-medium text-content/80">{agentName}</span>
            ) : durationMs != null ? (
              formatWorkingDuration(durationMs, turnModelName, true)
            ) : inlineWork && turnModelName ? (
              turnModelName
            ) : (
              workSummaryLine(summarizedWork)
            );
          const showFoldLine =
            !!habit ||
            standaloneReply ||
            live ||
            durationMs != null ||
            (inlineWork
              ? items.some(
                  (item) => item.type !== "block" || item.block.role !== "user",
                )
              : !!fold);
          // It sits where the work starts, from before there is any: the row
          // is there from the first token, so nothing shoves the answer down
          // when the turn folds.
          const firstWork =
            habit || standaloneReply
              ? 0
              : inlineWork
                ? items.findIndex(
                    (item) =>
                      item.type !== "block" || item.block.role !== "user",
                  )
                : firstFoldableIndex(items);
          const foldLineAt = fold
            ? fold.start
            : firstWork >= 0
              ? firstWork
              : items.length;
          const isCurrentItem = (item: TurnItem) =>
            item.type === "block"
              ? item.block.id === searchCurrent
              : item.blocks.some((block) => block.id === searchCurrent);
          const isCompactFollowUp = (item: TurnItem, index: number) => {
            const next = items[index + 1];
            return (
              inlineWork &&
              item.type === "block" &&
              item.block.role === "user" &&
              next?.type === "block" &&
              next.block.role === "user"
            );
          };
          const renderItem = (item: TurnItem, itemIndex: number) =>
            item.type === "subagents" && !inlineWork ? (
              <SubagentStack
                key={item.blocks[0].id}
                blocks={item.blocks}
                cwd={cwd}
                live={live}
                onOpenFile={onOpenFile}
                onOpenDiff={onOpenDiff}
              />
            ) : item.type !== "block" ? (
              inlineWork ? (
                <MonoWorkApprovals
                  key={item.blocks[0].id}
                  blocks={item.blocks}
                  cwd={cwd}
                  onApproval={onApproval}
                  onOpenFile={onOpenFile}
                  onOpenDiff={onOpenDiff}
                />
              ) : itemIndex === initialThinkingAt ? (
                <InitialThinking
                  key={item.blocks[0].id}
                  live={visible && !settled}
                />
              ) : (
                <ActivityPhases
                  key={item.blocks[0].id}
                  blocks={item.blocks}
                  cwd={cwd}
                  done={
                    !visible ||
                    settled ||
                    itemIndex < foldedAt ||
                    (answering && !workStillRunning)
                  }
                  onApproval={onApproval}
                  onOpenFile={onOpenFile}
                  onOpenDiff={onOpenDiff}
                />
              )
            ) : (
              <TranscriptBlock
                key={item.block.id}
                block={item.block}
                revealOnMount={
                  visible &&
                  wasVisible.current &&
                  !historicalBlockIds?.has(item.block.id) &&
                  !seenBlocks.current!.has(item.block.id)
                }
                layout={transcriptLayout}
                bubbleTail={!!agentMascot}
                compactFollowUp={isCompactFollowUp(item, itemIndex)}
                delivery={messageDeliveries?.get(item.block.id)}
                onRetryMessage={onRetryMessage}
                visible={item.block.role === "user" ? visible : undefined}
                stickyIndex={firstVisibleTurn + turnIndex + 1}
                // Prose reads the same wherever it lands: under the fold
                // line at the top of the turn, or under the work it follows.
                underLine={
                  isProseBlock(item.block) &&
                  (items[itemIndex - 1]?.type === "activity" ||
                    items[itemIndex - 1]?.type === "subagents" ||
                    (itemIndex === foldLineAt && showFoldLine))
                }
                onApproval={onApproval}
                onSaveNote={onSaveNote}
                onSendDraft={onSendDraft}
                onRemoveDraft={onRemoveDraft}
                onOpenFile={onOpenFile}
                onOpenDiff={onOpenDiff}
                onOpenPlan={onOpenPlan}
                onBuildPlan={onBuildPlan}
                planBusy={!!busy}
                planHarness={planBuildTargets ? harness : undefined}
                planModel={model}
                planModelSettings={modelSettings}
                cwd={cwd}
                onEditLastTurn={
                  onEditLastTurn &&
                  settled &&
                  item.block.role === "user" &&
                  item.block.id === editableUserBlockId &&
                  !item.block.draft
                    ? onEditLastTurn
                    : undefined
                }
                editing={
                  editingLastTurn &&
                  item.block.role === "user" &&
                  item.block.id === editableUserBlockId
                }
              />
            );
          // The fold reaches across a stack of delegated runs, but those rows
          // do not collapse with it: they are lifted out and parked under the
          // work, where they stay put however often it re-folds.
          const foldEntries = fold
            ? items.slice(fold.start, fold.end + 1).map((entry, offset) => ({
                entry,
                index: fold.start + offset,
              }))
            : [];
          const foldSubagents = foldEntries.filter(
            ({ entry }) => entry.type === "subagents",
          );
          const foldWork = foldEntries.filter(
            ({ entry }) => entry.type !== "subagents",
          );
          const foldLineRow = (
            <TurnRow key="work-fold" folded={!showFoldLine}>
              {inlineWork ? (
                <MonoTurnHeader
                  blocks={turn}
                  title={foldTitle}
                  name={turnModelName}
                  harness={turnHarness}
                  agentMascot={agentMascot}
                  active={!settled}
                  live={live}
                  waitingForAnswers={!!pendingQuestion}
                  backgroundTasks={backgroundTasks}
                  onShowWork={
                    onShowWork ? () => onShowWork(turnId, turn) : undefined
                  }
                  workExpanded={activeWorkTurnId === turnId}
                  searchCurrent={
                    turn.some((block) => block.id === searchCurrent) &&
                    !items.some((item) =>
                      item.type === "block"
                        ? item.block.id === searchCurrent
                        : item.blocks.some(
                            (block) =>
                              block.id === searchCurrent &&
                              needsApproval(block),
                          ),
                    )
                  }
                />
              ) : (
                <WorkFoldLine
                  title={foldTitle}
                  kind={workKind(summarizedWork)}
                  harness={turnHarness}
                  agentMascot={agentMascot}
                  live={live}
                  expandable={!!fold}
                  open={workOpen && !!fold}
                  onToggle={() => toggleWork(turnId, workOpen)}
                />
              )}
            </TurnRow>
          );
          return (
            <div
              key={turn[0].id}
              data-transcript-turn={turnId}
              className={`transcript-turn flex min-w-0 flex-col${
                isLastTurn ? " transcript-turn-live" : ""
              }${
                promptAnchor && anchorTurn && isLastTurn && userBlock
                  ? " transcript-turn-anchor"
                  : ""
              }`}
            >
              {stampAt != null ? <DaySeparator at={stampAt} /> : null}
              {items
                .flatMap((item, itemIndex) => {
                  // The header carries the process; only approvals need a row
                  // in the chat. The full turn remains in the activity sidebar.
                  if (
                    inlineWork &&
                    item.type !== "block" &&
                    !item.blocks.some(needsApproval)
                  )
                    return itemIndex === foldLineAt ? [foldLineRow] : [];
                  const inFold =
                    !!fold && itemIndex >= fold.start && itemIndex <= fold.end;
                  if (inFold) {
                    if (itemIndex !== fold.start) return [];
                    return [
                      foldLineRow,
                      <TurnRow key="work-details" folded={!workOpen}>
                        {() =>
                          foldWork.map(({ entry, index }, offset) => (
                            <div
                              key={turnItemKey(entry)}
                              data-transcript-search-item
                              data-transcript-search-current={
                                isCurrentItem(entry) || undefined
                              }
                              className={`flow-root pb-1 last:pb-0 pl-5 zen-fold-rail ${
                                offset === foldWork.length - 1
                                  ? "zen-fold-tail"
                                  : ""
                              }${
                                // Prose the trail holds is the agent talking
                                // while it works; the marker lets it read as
                                // process, not result.
                                entry.type === "block" &&
                                isProseBlock(entry.block)
                                  ? " zen-fold-prose"
                                  : ""
                              }`}
                            >
                              {renderItem(entry, index)}
                            </div>
                          ))
                        }
                      </TurnRow>,
                      // Delegated runs sit under the agent's own work, not
                      // among it: they are a second thing the turn is doing,
                      // and reading them as the first steps of the main trail
                      // is what made them look like its work.
                      ...foldSubagents.map(({ entry, index }) => (
                        <div
                          key={turnItemKey(entry)}
                          data-transcript-search-item
                          data-transcript-search-current={
                            isCurrentItem(entry) || undefined
                          }
                          className="flow-root pb-1"
                        >
                          {renderItem(entry, index)}
                        </div>
                      )),
                    ];
                  }
                  const row = (
                    <div
                      key={turnItemKey(item)}
                      data-transcript-search-item
                      data-transcript-search-current={
                        isCurrentItem(item) || undefined
                      }
                      className={`flow-root ${isCompactFollowUp(item, itemIndex) ? "pb-0" : "pb-1"}`}
                    >
                      {renderItem(item, itemIndex)}
                    </div>
                  );
                  if (itemIndex !== foldLineAt) return row;
                  return [foldLineRow, row];
                })
                .concat(foldLineAt >= items.length ? [foldLineRow] : [])}
              {settled &&
                proposals
                  .filter((block) => block.orchestration?.status !== "planning")
                  .map((block) => (
                    <div
                      key={block.id}
                      className="px-4 pt-1 pb-2"
                      data-orchestration-result
                    >
                      <OrchestrationPreview block={block} busy={!!busy} />
                    </div>
                  ))}
              {settled && artifacts.length > 0 ? (
                <div
                  data-artifact-results
                  className="flex flex-col gap-2 px-4 pt-1 pb-3"
                >
                  {artifacts.map((card) => (
                    <ArtifactCard
                      key={card.id}
                      card={card}
                      onOpen={onOpenArtifact}
                    />
                  ))}
                </div>
              ) : null}
              {/* The accessory keeps the pane's props, which go stale once parked. */}
              {isLastTurn && latestTurnAccessory && !parked
                ? latestTurnAccessory
                : null}
              {settled &&
              (durationMs != null ||
                standaloneReply ||
                (inlineWork && firstWork >= 0) ||
                (spawnedSessions.length > 0 && onShowSessions)) ? (
                <TurnDuration
                  elapsedMs={durationMs ?? null}
                  label={
                    standaloneReply ? (agentName ?? habit?.name) : undefined
                  }
                  metrics={hideTurnMetrics ? undefined : userBlock?.turnMetrics}
                  labelHidden={showFoldLine}
                  modelName={turnModelName}
                  completedAt={
                    habit?.at ??
                    (runs.length > 1 && lastRun?.startedAt != null
                      ? lastRun.startedAt + (lastRun.durationMs ?? 0)
                      : startedAt != null
                        ? startedAt + (durationMs ?? 0)
                        : undefined)
                  }
                  copyText={turnCopyText(
                    inlineWork
                      ? items.flatMap((item) =>
                          item.type === "block" ? [item.block] : [],
                        )
                      : turn,
                  )}
                  onSaveNote={onSaveNote}
                  onShowWork={
                    inlineWork && onShowWork
                      ? () => onShowWork(turnId, turn)
                      : undefined
                  }
                  workExpanded={activeWorkTurnId === turnId}
                  onShowSessions={
                    spawnedSessions.length && onShowSessions
                      ? () => onShowSessions(turnId, turn)
                      : undefined
                  }
                  sessionsExpanded={activeSessionsTurnId === turnId}
                  sessionCount={spawnedSessions.length}
                  harness={turnHarness}
                  fromHarness={turnHarness}
                  fromModel={turnModel?.id}
                  onSecondOpinion={
                    onSecondOpinion
                      ? (target) => onSecondOpinion(target, turn)
                      : undefined
                  }
                  onHandoff={
                    onHandoff ? (target) => onHandoff(target, turn) : undefined
                  }
                />
              ) : null}
            </div>
          );
        })}
      </TranscriptContent>
      {onAddToChat || onSaveSelectionNote ? (
        <TranscriptSelectionMenu
          selection={selection}
          onAddToChat={onAddToChat}
          onAddToNotes={onSaveSelectionNote}
          onDismiss={dismissSelection}
        />
      ) : null}
    </div>
  );
}

function TranscriptContent({
  bottomAligned,
  children,
}: {
  bottomAligned: boolean;
  children: ReactNode;
}) {
  if (!bottomAligned) {
    return (
      <div
        data-transcript-content
        className="mx-auto flex w-full min-w-0 max-w-4xl flex-col gap-1 pb-8"
      >
        {children}
      </div>
    );
  }
  return (
    <div className="mx-auto flex min-h-full w-full min-w-0 max-w-4xl flex-col justify-end overflow-clip pb-8">
      <div
        data-transcript-content
        className="flex min-w-0 shrink-0 flex-col gap-1"
      >
        {children}
      </div>
    </div>
  );
}

// Keep hidden panes' local state, and catch up with current props on activation.
export const AgentTranscript = memo(
  AgentTranscriptComponent,
  (previous, next) => previous.visible === false && next.visible === false,
);

/** "Today", "Yesterday" or the date, in bold, with the time beside it. */
export function dayStamp(at: number, now = Date.now()): [string, string] {
  const date = new Date(at);
  const today = new Date(now);
  const yesterday = new Date(now);
  yesterday.setDate(today.getDate() - 1);
  const day =
    date.toDateString() === today.toDateString()
      ? "Today"
      : date.toDateString() === yesterday.toDateString()
        ? "Yesterday"
        : new Intl.DateTimeFormat(undefined, {
            weekday: "short",
            month: "short",
            day: "numeric",
            ...(date.getFullYear() === today.getFullYear()
              ? {}
              : { year: "numeric" }),
          }).format(at);
  return [day, formatClockTime(at)];
}

function DaySeparator({ at }: { at: number }) {
  const [day, time] = dayStamp(at);
  return (
    <div
      role="separator"
      aria-label={`${day} ${time}`}
      data-day-separator
      className="flex items-center justify-center px-4 pb-3 pt-6 font-sans text-[12px]"
    >
      <span className="rounded-full bg-content/[0.07] px-3 py-1 leading-4">
        <span className="font-semibold text-content/75">{day}</span>{" "}
        <span className="text-content/45">{time}</span>
      </span>
    </div>
  );
}

/** Placeholder for private reasoning before the first assistant text arrives. */
function InitialThinking({
  live,
  embedded = false,
}: {
  live: boolean;
  embedded?: boolean;
}) {
  return (
    <div
      className={`min-w-0 pt-3 pb-1 font-sans text-sm text-content/50 ${embedded ? "" : "px-4"}`}
    >
      {live ? <Shimmer duration={1.6}>Thinking…</Shimmer> : "Thinking…"}
    </div>
  );
}

/**
 * The clock on a turn's fold line: how long the agent has been at it, or what
 * it is waiting on. The band that sweeps the text is sized off this element,
 * so it shrinks to the words — stretched across the row, the sweep spends its
 * time on empty space and the line just sits there looking dim.
 */
function LiveFoldTitle({
  startedAt,
  paused,
  waitingLabel,
  background,
  modelName,
}: {
  startedAt?: number;
  paused: boolean;
  waitingLabel?: string;
  background?: string[];
  modelName?: string;
}) {
  const elapsedMs = useElapsedFrom(startedAt, paused);
  // Yielding with a command still going is not the end of the turn. The clock
  // keeps running and the line says what it is waiting on.
  const text = paused
    ? (waitingLabel ?? "Waiting for approval")
    : background?.length
      ? `${formatWorkingDuration(elapsedMs, modelName)} · ${backgroundLabel(background)}`
      : formatWorkingDuration(elapsedMs, modelName);
  const shimmer = (
    <Shimmer className="min-w-0 truncate font-sans text-sm" duration={1}>
      {text}
    </Shimmer>
  );
  return background?.length ? (
    <span className="flex min-w-0" title={background.join("\n")}>
      {shimmer}
    </span>
  ) : (
    shimmer
  );
}

function backgroundLabel(tasks: string[]): string {
  return tasks.length === 1
    ? "running in background"
    : `${tasks.length} tasks running in background`;
}

/**
 * What a finished turn leaves under the answer: what you can do with it, and
 * when it landed. The clock lives on the fold line above, from the first token
 * to the last, so it is not repeated here.
 */
function TurnDuration({
  elapsedMs,
  label: completionLabel,
  metrics,
  labelHidden = false,
  modelName,
  harness,
  completedAt,
  copyText: output,
  onSaveNote,
  onShowWork,
  workExpanded,
  onShowSessions,
  sessionsExpanded,
  sessionCount,
  fromHarness,
  fromModel,
  onSecondOpinion,
  onHandoff,
}: {
  elapsedMs: number | null;
  label?: string;
  metrics?: TurnMetrics;
  /** True when the fold line above already keeps the time for this turn. */
  labelHidden?: boolean;
  modelName?: string;
  harness?: HarnessId;
  completedAt?: number;
  copyText?: string;
  onSaveNote?: (text: string) => void | Promise<void>;
  onShowWork?: () => void;
  workExpanded?: boolean;
  onShowSessions?: () => void;
  sessionsExpanded?: boolean;
  sessionCount?: number;
  fromHarness?: HarnessId;
  /** The turn's own model, so a same-harness second opinion can hide it. */
  fromModel?: string;
  onSecondOpinion?: (target: ModelTarget) => void;
  onHandoff?: (target: ModelTarget) => void;
}) {
  const label =
    completionLabel ?? formatWorkingDuration(elapsedMs, modelName, true);
  const dot = (
    <span
      aria-hidden
      className="size-[3px] shrink-0 rounded-full bg-content/25"
    />
  );
  return (
    <div
      data-turn-actions
      aria-label={label}
      className="flex w-full min-w-0 max-w-full items-center gap-2.5 overflow-hidden px-4 pt-1 pb-3 font-sans text-sm text-content/40"
    >
      <span className="flex shrink-0 items-center gap-1">
        {output ? (
          <>
            <CopyTurnButton text={output} />
            {onSaveNote ? (
              <SaveNoteButton text={output} onSave={onSaveNote} />
            ) : null}
          </>
        ) : (
          <Check className="size-3.5" strokeWidth={1.75} />
        )}
        {onShowWork ? (
          <button
            type="button"
            title={workExpanded ? "Hide activity" : "Show activity"}
            aria-label={workExpanded ? "Hide activity" : "Show activity"}
            aria-expanded={!!workExpanded}
            onClick={onShowWork}
            className={`rounded-md p-1 outline-none hover:bg-content/8 hover:text-content/70 focus-visible:ring-1 focus-visible:ring-accent ${workExpanded ? "bg-content/8 text-content/70" : "text-content/40"}`}
          >
            <ListBullet className="size-3.5" strokeWidth={1.75} />
          </button>
        ) : null}
        {onShowSessions ? (
          <button
            type="button"
            title={`${sessionsExpanded ? "Hide" : "Show"} sessions (${sessionCount})`}
            aria-label={sessionsExpanded ? "Hide sessions" : "Show sessions"}
            aria-expanded={!!sessionsExpanded}
            onClick={onShowSessions}
            className={`flex items-center gap-1 rounded-md p-1 outline-none hover:bg-content/8 hover:text-content/70 focus-visible:ring-1 focus-visible:ring-accent ${sessionsExpanded ? "bg-content/8 text-content/70" : "text-content/40"}`}
          >
            <Chatting className="size-3.5" strokeWidth={1.75} />
            <span className="text-[11px] leading-none">{sessionCount}</span>
          </button>
        ) : null}
        {fromHarness && onHandoff ? (
          <HandoffButton from={fromHarness} onPick={onHandoff} />
        ) : null}
        {fromHarness && onSecondOpinion ? (
          <SecondOpinionButton
            from={fromHarness}
            fromModel={fromModel}
            onPick={onSecondOpinion}
            includeCurrent
            excludeFromModel
          />
        ) : null}
        <TurnMetricsBadge metrics={metrics} elapsedMs={elapsedMs} />
      </span>
      {labelHidden ? null : (
        <span className="flex min-w-0 items-center gap-2.5">
          {dot}
          <span className="flex min-w-0 items-center gap-1.5">
            {harness ? (
              <HarnessIcon harness={harness} className="size-3.5 shrink-0" />
            ) : null}
            <span className="min-w-0 truncate" title={label}>
              {label}
            </span>
          </span>
        </span>
      )}
      {completedAt != null ? (
        <span className="flex shrink-0 items-center gap-2.5">
          {dot}
          <span className="shrink-0 text-content/35">
            {formatClockTime(completedAt)}
          </span>
        </span>
      ) : null}
    </div>
  );
}

function TurnMetricsBadge({
  metrics,
  elapsedMs,
}: {
  metrics?: TurnMetrics;
  elapsedMs: number | null;
}) {
  const root = useRef<HTMLDivElement>(null);
  const [hovered, setHovered] = useState(false);
  if (!metrics || !hasTurnMetrics(metrics)) return null;

  const outputRate =
    metrics.outputTokens != null && elapsedMs != null && elapsedMs > 0
      ? metrics.outputTokens / (elapsedMs / 1000)
      : undefined;
  const headline =
    [
      metrics.cacheHitPercent != null
        ? `Cache hit ${Math.round(metrics.cacheHitPercent)}%`
        : null,
      outputRate != null
        ? `Output ${formatMetricCount(outputRate)} tok/s`
        : null,
    ]
      .filter(Boolean)
      .join(" · ") || "Turn tokens";
  const detail = [
    metrics.inputTokens != null
      ? `${formatMetricCount(metrics.inputTokens)} input`
      : null,
    metrics.outputTokens != null
      ? `${formatMetricCount(metrics.outputTokens)} output`
      : null,
    metrics.cacheReadTokens != null
      ? `${formatMetricCount(metrics.cacheReadTokens)} cached`
      : null,
  ]
    .filter(Boolean)
    .join(" · ");
  const label = [headline, detail].filter(Boolean).join(". ");

  return (
    <div
      ref={root}
      className="relative shrink-0 ml-[3px]"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onFocus={() => setHovered(true)}
      onBlur={() => setHovered(false)}
    >
      <span
        role="img"
        tabIndex={0}
        aria-label={`Turn metrics: ${label}`}
        title="Turn metrics"
        className="grid rounded-md p-1 text-content/40 outline-none hover:bg-content/8 hover:text-content/70 focus-visible:ring-1 focus-visible:ring-accent"
      >
        <ChartBreakoutSquare className="size-3.5" strokeWidth={1.75} />
      </span>
      {hovered ? (
        <Popover
          anchor={root}
          side="top"
          align="start"
          className="pointer-events-none w-max px-2.5 py-1.5"
        >
          <div className="text-[12px] leading-4 text-content">{headline}</div>
          {detail ? (
            <div className="text-[11px] leading-4 text-content/50">
              {detail}
            </div>
          ) : null}
        </Popover>
      ) : null}
    </div>
  );
}

function hasTurnMetrics(metrics: TurnMetrics): boolean {
  return (
    metrics.cacheHitPercent != null ||
    (metrics.inputTokens ?? 0) > 0 ||
    (metrics.outputTokens ?? 0) > 0 ||
    (metrics.cacheReadTokens ?? 0) > 0 ||
    (metrics.cacheWriteTokens ?? 0) > 0
  );
}

function formatMetricCount(value: number): string {
  return new Intl.NumberFormat(undefined, {
    notation: "compact",
    maximumFractionDigits: value >= 1000 ? 1 : 0,
  }).format(Math.max(0, Math.round(value)));
}

/** Wall-clock stamp for a finished turn, in the reader's own locale. */
function formatClockTime(epochMs: number): string {
  return new Date(epochMs).toLocaleTimeString(undefined, {
    hour: "numeric",
    minute: "2-digit",
  });
}

function CopyTurnButton({
  text,
  attachments,
  label = "Copy response",
}: {
  text: string;
  attachments?: Attachment[];
  label?: string;
}) {
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | null>(null);

  useEffect(() => {
    setCopied(false);
    setError(null);
    return () => {
      if (timer.current != null) window.clearTimeout(timer.current);
    };
  }, [text, attachments]);

  return (
    <>
      <button
        type="button"
        disabled={pending}
        title={copied ? "Copied" : label}
        aria-label={copied ? "Copied" : label}
        className="-ml-1 rounded-md p-1 text-content/40 hover:bg-content/8 hover:text-content/70"
        onClick={(event) => {
          event.stopPropagation();
          setError(null);
          setCopied(false);
          setPending(true);
          playCue("copy");
          void copyMessage(text, attachments).then(
            () => {
              setPending(false);
              setCopied(true);
              if (timer.current != null) window.clearTimeout(timer.current);
              timer.current = window.setTimeout(() => setCopied(false), 2000);
            },
            (error: unknown) => {
              setPending(false);
              setError(error instanceof Error ? error.message : String(error));
            },
          );
        }}
      >
        {copied ? (
          <Check className="size-3.5" strokeWidth={1.75} />
        ) : (
          <Copy className="size-3.5" strokeWidth={1.75} />
        )}
      </button>
      {error && (
        <span role="alert" className="max-w-xs text-xs text-content/70">
          Copy failed. {error}
        </span>
      )}
    </>
  );
}

function SaveNoteButton({
  text,
  onSave,
}: {
  text: string;
  onSave: (text: string) => void | Promise<void>;
}) {
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [saved, setSaved] = useState(false);
  const timer = useRef<number | null>(null);

  useEffect(() => {
    setSaved(false);
    setError(null);
    return () => {
      if (timer.current != null) window.clearTimeout(timer.current);
    };
  }, [text]);

  return (
    <>
      <button
        type="button"
        disabled={pending}
        title={saved ? "Saved to Notes" : "Save as note"}
        aria-label={saved ? "Saved to Notes" : "Save as note"}
        className="rounded-md p-1 text-content/40 hover:bg-content/8 hover:text-content/70"
        onClick={async () => {
          setError(null);
          setSaved(false);
          setPending(true);
          try {
            await onSave(text);
            playCue("copy");
            setSaved(true);
            if (timer.current != null) window.clearTimeout(timer.current);
            timer.current = window.setTimeout(() => setSaved(false), 2000);
          } catch (error) {
            setError(error instanceof Error ? error.message : String(error));
          } finally {
            setPending(false);
          }
        }}
      >
        {saved ? (
          <Check className="size-3.5" strokeWidth={1.75} />
        ) : (
          <FilePlusCorner className="size-3.5" strokeWidth={1.75} />
        )}
      </button>
      {error && (
        <span role="alert" className="max-w-xs text-xs text-content/70">
          Could not save note. {error}
        </span>
      )}
    </>
  );
}

function EditLastTurnButton({
  onEdit,
  editing = false,
}: {
  onEdit: () => void;
  editing?: boolean;
}) {
  const label = editing ? "Cancel edit" : "Edit and resend";
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={editing}
      onClick={(event) => {
        event.stopPropagation();
        onEdit();
      }}
      className={`rounded-md p-1 transition-[background-color,color] duration-150 focus-visible:ring-1 focus-visible:ring-accent ${
        editing
          ? "edit-last-turn-button"
          : "text-content/40 hover:bg-content/8 hover:text-content/70"
      }`}
    >
      <Pencil className="size-3.5" strokeWidth={1.75} />
    </button>
  );
}

const TranscriptBlock = memo(function TranscriptBlock({
  block,
  layout,
  bubbleTail = false,
  compactFollowUp = false,
  delivery,
  onRetryMessage,
  visible,
  revealOnMount,
  stickyIndex,
  underLine = false,
  embedded = false,
  cwd,
  onApproval,
  onSaveNote,
  onSendDraft,
  onRemoveDraft,
  onOpenFile,
  onOpenDiff,
  onOpenPlan,
  onBuildPlan,
  planBusy,
  planHarness,
  planModel,
  planModelSettings,
  onEditLastTurn,
  editing = false,
}: {
  block: Block;
  layout: TranscriptLayout;
  bubbleTail?: boolean;
  compactFollowUp?: boolean;
  delivery?: MessageDelivery;
  onRetryMessage?: (blockId: string) => void;
  visible?: boolean;
  revealOnMount?: boolean;
  stickyIndex: number;
  /** True when something already sits directly above this in the turn. */
  underLine?: boolean;
  /** True when the parent surface already provides the horizontal gutter. */
  embedded?: boolean;
  cwd?: string;
  onApproval?: (requestId: number, decision: ApprovalDecision) => void;
  onSaveNote?: (text: string) => void | Promise<void>;
  onSendDraft?: (block: Block) => boolean | void;
  onRemoveDraft?: (block: Block) => boolean | void;
  onOpenFile?: (path: string) => void;
  onOpenDiff?: (path: string) => void;
  onOpenPlan?: (blockId: string) => void;
  onBuildPlan?: (blockId: string, target?: PlanBuildTarget) => void;
  planBusy?: boolean;
  planHarness?: HarnessId;
  planModel?: string;
  planModelSettings?: Record<string, string>;
  onEditLastTurn?: () => void;
  editing?: boolean;
}) {
  if (block.role === "user") {
    return (
      <UserMessageBlock
        block={block}
        layout={layout}
        bubbleTail={bubbleTail}
        compactFollowUp={compactFollowUp}
        delivery={delivery}
        onRetryMessage={onRetryMessage}
        visible={visible ?? true}
        stickyIndex={stickyIndex}
        cwd={cwd}
        onEdit={onEditLastTurn}
        editing={editing}
        onSaveNote={onSaveNote}
        onSendDraft={onSendDraft}
        onRemoveDraft={onRemoveDraft}
      />
    );
  }

  if (block.role === "image") {
    return block.image ? <GeneratedImage image={block.image} /> : null;
  }

  if (block.role === "tool") {
    return (
      <ToolCall
        block={block}
        cwd={cwd}
        embedded={embedded}
        onApproval={onApproval}
        onOpenFile={onOpenFile}
        onOpenDiff={onOpenDiff}
      />
    );
  }

  if (block.role === "reasoning") {
    return null;
  }

  if (block.role === "tasks") {
    if (!block.taskList?.items.length) return null;
    return (
      <div className={embedded ? "py-1" : "px-4 py-1"}>
        <TaskListPreview
          items={block.taskList.items}
          explanation={block.taskList.explanation}
        />
      </div>
    );
  }

  if (block.role === "plan") {
    if (block.orchestration) return null;
    const legacyTasks = legacyTaskListFromText(block.text);
    if (legacyTasks) {
      return (
        <div className={embedded ? "py-1" : "px-4 py-1"}>
          <TaskListPreview items={legacyTasks} />
        </div>
      );
    }
    return (
      <div className={embedded ? "py-1" : "px-4 py-1"}>
        <PlanPreview
          text={block.text}
          streaming={block.streaming}
          busy={planBusy}
          plan={block.plan}
          harness={planHarness}
          model={planModel}
          modelSettings={planModelSettings}
          onOpen={onOpenPlan ? () => onOpenPlan(block.id) : undefined}
          onBuild={
            onBuildPlan ? (target) => onBuildPlan(block.id, target) : undefined
          }
        />
      </div>
    );
  }

  if (block.role === "approval") {
    return (
      <ToolCall
        block={block}
        cwd={cwd}
        embedded={embedded}
        onApproval={onApproval}
        onOpenFile={onOpenFile}
        onOpenDiff={onOpenDiff}
      />
    );
  }

  if (block.role === "handoff") {
    return <HandoffDivider block={block} />;
  }

  if (block.role === "system") {
    if (block.interjection) {
      return <InterjectionDivider block={block} />;
    }
    return (
      <div className={`${embedded ? "" : "px-4"} py-2 text-content/50`}>
        <pre className="min-w-0 whitespace-pre-wrap break-words">
          {block.text}
        </pre>
      </div>
    );
  }

  if (!block.text && block.streaming) return null;

  return (
    <div
      data-selectable-agent-response={block.streaming ? undefined : block.id}
      data-chat-message={block.id}
      data-chat-message-role="assistant"
      className={`min-w-0 pb-1 text-content ${embedded ? "" : "px-4"} ${underLine ? "pt-1" : "pt-3"}`}
    >
      <AgentMarkdown
        text={block.text}
        streaming={block.streaming}
        revealOnMount={revealOnMount}
        cwd={cwd}
        onOpenFile={onOpenFile}
      />
    </div>
  );
});

function UserMessageBlock({
  block,
  layout,
  bubbleTail = false,
  compactFollowUp = false,
  delivery,
  onRetryMessage,
  visible,
  stickyIndex,
  onEdit,
  editing = false,
  cwd,
  onSaveNote,
  onSendDraft,
  onRemoveDraft,
}: {
  block: Block;
  layout: TranscriptLayout;
  bubbleTail?: boolean;
  compactFollowUp?: boolean;
  delivery?: MessageDelivery;
  onRetryMessage?: (blockId: string) => void;
  visible: boolean;
  stickyIndex: number;
  onEdit?: () => void;
  editing?: boolean;
  cwd?: string;
  onSaveNote?: (text: string) => void | Promise<void>;
  onSendDraft?: (block: Block) => boolean | void;
  onRemoveDraft?: (block: Block) => boolean | void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [overflows, setOverflows] = useState(false);
  const [singleLine, setSingleLine] = useState(false);
  const textRef = useRef<HTMLElement>(null);
  const card = block.secondOpinion;
  const note = block.noteCard;
  const monocode = isOperatorUserTurn(block);
  const text =
    card && card.kind !== "handoff"
      ? ""
      : visibleUserPrompt(monocode ? operatorUserPrompt(block) : block.text);
  const messageLink = text ? parseUserMessageLink(text) : null;
  const displayText = messageLink
    ? `${messageLink.beforeText}${messageLink.afterText}`
    : text;
  const chat = layout === "chat";
  const messageAt = block.sentAt ?? block.startedAt;
  const chatSpacing = compactFollowUp
    ? "pt-1 pb-0"
    : bubbleTail && block.sentAt != null
      ? "pt-1 pb-5"
      : "pt-1.5 pb-5";
  // A Mono's chat shows sent images above the bubble, like photos in a
  // messaging app; only other files stay inside it.
  const photos =
    bubbleTail && chat && !block.draft
      ? (block.attachments ?? []).filter(
          (file) => file.kind === "image" && attachmentPreviewSrc(file),
        )
      : [];
  const bubbleAttachments = photos.length
    ? block.attachments!.filter((file) => !photos.includes(file))
    : block.attachments;
  const textOnly =
    Boolean(text) &&
    !block.draft &&
    !bubbleAttachments?.length &&
    !card &&
    !note &&
    !block.ciContext;
  // Images sent on their own need no bubble beneath them.
  const bubbleEmpty =
    photos.length > 0 &&
    !text &&
    !bubbleAttachments?.length &&
    !card &&
    !note &&
    !block.ciContext;

  // Only the chat layout rounds a single line; the document layout always uses
  // the square corners, so it never needs the measurement at all.
  const roundsSingleLine = chat && textOnly;

  useLayoutEffect(() => {
    if (!visible) return;
    const el = textRef.current;
    if (!el || !text) {
      setOverflows(false);
      setSingleLine(false);
      return;
    }

    // Every user message carries one of these observers, so the callback runs
    // once per message whenever the transcript reflows. `getComputedStyle`
    // forces a style recalculation on each call and the line height only moves
    // with the font or the UI scale — never with a resize — so it is resolved
    // once here rather than on every delivery.
    let lineHeight = 0;
    const measure = () => {
      // Reading a descendant's size makes the browser lay out an otherwise
      // skipped historical turn. Leave it skipped until it comes into view.
      if (
        !el.isConnected ||
        (el.checkVisibility &&
          !el.checkVisibility({ contentVisibilityAuto: true }))
      )
        return;
      if (!expanded) {
        setOverflows(el.scrollHeight > el.clientHeight + 1);
      }
      if (!roundsSingleLine) {
        setSingleLine(false);
        return;
      }
      // Pooled or offscreen turns can measure as zero before they are laid out.
      if (el.clientWidth === 0) {
        setSingleLine(false);
        return;
      }
      if (!lineHeight) {
        lineHeight = Number.parseFloat(getComputedStyle(el).lineHeight);
      }
      setSingleLine(
        Number.isFinite(lineHeight) && el.scrollHeight <= lineHeight + 1,
      );
    };

    const turn = el.closest(".transcript-turn");
    const onVisible = (event: Event) => {
      if (!(event as ContentVisibilityAutoStateChangeEvent).skipped) measure();
    };
    turn?.addEventListener("contentvisibilityautostatechange", onVisible);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => {
      observer.disconnect();
      turn?.removeEventListener("contentvisibilityautostatechange", onVisible);
    };
  }, [text, roundsSingleLine, expanded, visible]);

  const toggle = () => {
    if (overflows) setExpanded((value) => !value);
  };

  const deliveryControl =
    delivery && delivery.status !== "pending" ? (
      <div className="flex max-w-full items-center gap-2 px-3 pt-1 font-sans text-xs text-content/50">
        <span role={delivery.error ? "alert" : "status"}>
          {delivery.error ? `Not sent · ${delivery.error}` : "Not sent"}
        </span>
        {onRetryMessage ? (
          <button
            type="button"
            onClick={() => onRetryMessage(block.id)}
            className="shrink-0 rounded px-1 py-0.5 text-content/70 hover:bg-content/8"
          >
            {delivery.status === "failed" ? "Retry" : "Resume"}
          </button>
        ) : null}
      </div>
    ) : null;
  // In a Mono's chat a few emoji stand alone, like a reaction: no bubble,
  // time or actions.
  if (
    bubbleTail &&
    chat &&
    textOnly &&
    !monocode &&
    !messageLink &&
    isEmojiOnlyMessage(displayText)
  ) {
    return (
      <div
        data-prompt-anchor={block.id}
        data-message-delivery={delivery?.status}
        data-message-stack={compactFollowUp ? "true" : undefined}
        className={`user-message-row flex flex-col items-end pr-4 pl-14 ${chatSpacing}`}
      >
        <div
          data-chat-message={block.id}
          data-chat-message-role="user"
          className="select-none font-sans text-6xl leading-none"
        >
          {displayText.trim()}
        </div>
        {deliveryControl}
      </div>
    );
  }

  return (
    <div
      data-prompt-anchor={block.id}
      data-message-delivery={delivery?.status}
      data-message-stack={compactFollowUp ? "true" : undefined}
      data-editing-last-turn={editing ? "true" : undefined}
      className={`user-message-row group/usermsg overflow-visible ${
        chat
          ? `flex flex-col items-end pr-4 pl-14 ${chatSpacing}`
          : "p-1.5 pb-4"
      }`}
    >
      <div
        className={`user-message-hover-zone min-w-0 overflow-visible ${chat ? "flex w-fit max-w-full flex-col items-end" : "w-full"}`}
      >
        {/* Photos and their bubble enter as one message. */}
        <div
          data-chat-message={photos.length ? block.id : undefined}
          data-chat-message-role={photos.length ? "user" : undefined}
          className={
            photos.length
              ? "flex min-w-0 max-w-full flex-col items-end gap-1"
              : "contents"
          }
        >
          {photos.length ? (
            <div className="flex max-w-full flex-wrap justify-end gap-1.5">
              {photos.map((file) => (
                <AttachmentChip key={file.id} attachment={file} photo />
              ))}
            </div>
          ) : null}
          {bubbleEmpty ? null : (
            <div
              data-draft={block.draft ? "true" : undefined}
              data-monocode={monocode ? "true" : undefined}
              data-chat-message={photos.length ? undefined : block.id}
              data-chat-message-role={photos.length ? undefined : "user"}
              className={`user-message-bubble relative min-w-0 px-3 py-2 font-sans text-content transition-[background-color] duration-200 ${
                block.draft
                  ? "border border-dashed border-content/30 bg-content/4"
                  : "bg-content/10"
              } ${editing ? "edit-last-turn-bubble" : ""} ${
                bubbleTail && chat && !block.draft && !compactFollowUp
                  ? `agent-chat-bubble-tail-right ${singleLine ? "[--agent-bubble-radius:1.125rem]" : "[--agent-bubble-radius:0.75rem]"}`
                  : ""
              } ${
                chat
                  ? `w-fit max-w-[min(100%,36rem)] ${singleLine ? "rounded-full" : "rounded-xl"}`
                  : "rounded-lg border border-content/10"
              }`}
              style={{ zIndex: stickyIndex }}
            >
              {bubbleAttachments?.length ? (
                <div
                  className={`flex flex-wrap gap-1.5 ${text || card || note ? "mb-2" : ""}`}
                >
                  {bubbleAttachments.map((file) => (
                    <AttachmentChip key={file.id} attachment={file} />
                  ))}
                </div>
              ) : null}
              {note ? (
                <div className={text || card ? "mb-2" : ""}>
                  <NoteMiniCard card={note} embedded />
                </div>
              ) : null}
              {card ? (
                <div className={text ? "mb-1.5" : undefined}>
                  <SecondOpinionCard card={card} />
                </div>
              ) : null}
              {messageLink ? (
                <div
                  ref={(element) => {
                    textRef.current = element;
                  }}
                  className="user-message-with-link min-w-0 whitespace-pre-wrap break-words font-sans text-sm"
                  data-selectable-agent-response={block.id}
                >
                  {messageLink.beforeText}
                  <UserLinkPreview link={messageLink.link} cwd={cwd} compact />
                  {messageLink.afterText}
                </div>
              ) : displayText ? (
                <pre
                  data-selectable-agent-response={block.id}
                  ref={(element) => {
                    textRef.current = element;
                  }}
                  className={`min-w-0 whitespace-pre-wrap break-words font-sans text-sm ${expanded ? "" : "line-clamp-4"}`}
                >
                  {displayText}
                </pre>
              ) : null}
              {overflows ? (
                <button
                  type="button"
                  aria-expanded={expanded}
                  className="mt-1 rounded px-1 py-0.5 text-xs text-content/60 hover:bg-content/8 hover:text-content"
                  onClick={toggle}
                >
                  {expanded ? "Show less" : "Show more"}
                </button>
              ) : null}
              {block.ciContext ? (
                <details
                  className="group/ci mt-2 min-w-0 border-t border-content/10 pt-2"
                  onClick={(event) => event.stopPropagation()}
                >
                  <summary className="flex w-fit cursor-pointer list-none items-center gap-1.5 rounded text-xs text-content/50 transition-colors hover:text-content/80 focus-visible:outline focus-visible:outline-1 focus-visible:outline-content/40 [&::-webkit-details-marker]:hidden">
                    <ChevronRight className="size-3 shrink-0 transition-transform group-open/ci:rotate-90" />
                    <span>CI context</span>
                  </summary>
                  <p className="mt-2 text-xs text-content/50">
                    CI instructions and failure details included with this
                    request.
                  </p>
                  <pre className="mt-2 max-h-72 min-w-0 overflow-auto overscroll-contain rounded-md bg-content/5 p-2.5 font-mono text-[11px] leading-relaxed whitespace-pre-wrap break-words text-content/70">
                    {block.ciContext}
                  </pre>
                </details>
              ) : null}
              {block.draft ? (
                <div className="mt-2 flex items-center justify-between gap-4 border-t border-dashed border-content/20 pt-2">
                  <span className="flex items-center gap-1.5 text-xs text-content/50">
                    <CircleDashed className="size-3.5" strokeWidth={1.75} />
                    Draft
                  </span>
                  <span className="flex items-center gap-1">
                    <button
                      type="button"
                      title="Remove draft"
                      aria-label="Remove draft"
                      onClick={() => onRemoveDraft?.(block)}
                      className="flex h-7 items-center gap-1.5 rounded-md px-2 text-xs text-content/55 hover:bg-content/10 hover:text-content"
                    >
                      <Trash2 className="size-3.5" strokeWidth={1.75} />
                      Remove
                    </button>
                    <button
                      type="button"
                      title="Send draft"
                      aria-label="Send draft"
                      onClick={() => onSendDraft?.(block)}
                      className="primary-action flex h-7 items-center gap-1.5 rounded-md px-2.5 text-xs font-medium transition-transform duration-150 active:scale-[0.97]"
                    >
                      Send
                      <ArrowUp className="size-3.5" strokeWidth={2.25} />
                    </button>
                  </span>
                </div>
              ) : null}
              {monocode ? (
                <MonocodeSparkles
                  blockId={block.id}
                  startedAt={block.startedAt}
                />
              ) : block.intent === "plan" ? (
                <PlanStepsBurst
                  blockId={block.id}
                  startedAt={block.startedAt}
                />
              ) : block.intent === "orchestrate" ? (
                <OrchestratorConstellation
                  blockId={block.id}
                  startedAt={block.startedAt}
                />
              ) : null}
            </div>
          )}
        </div>
        {deliveryControl}
        {!compactFollowUp &&
        (text || block.attachments?.length || messageAt != null || onEdit) ? (
          <div className="user-message-actions flex items-center gap-1 px-3 pt-1">
            {text || block.attachments?.length ? (
              <CopyTurnButton
                text={text}
                attachments={block.attachments}
                label="Copy message"
              />
            ) : null}
            {onEdit ? (
              <EditLastTurnButton onEdit={onEdit} editing={editing} />
            ) : null}
            {text && onSaveNote ? (
              <SaveNoteButton text={text} onSave={onSaveNote} />
            ) : null}
            {messageAt != null ? (
              <time
                dateTime={new Date(messageAt).toISOString()}
                title={new Date(messageAt).toLocaleString()}
                className="ml-1 font-sans text-xs text-content/40"
              >
                {formatClockTime(messageAt)}
              </time>
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  );
}

/**
 * Animate one fold, then release its contents. Closed work must not retain
 * a component and DOM tree for every tool; only expansion builds those rows.
 * Visible rows stay out of Grid so they rewrap when their pane changes width.
 */
function TurnRow({
  folded,
  children,
}: {
  folded: boolean;
  children: ReactNode | (() => ReactNode);
}) {
  const [foldState, setFoldState] = useState<
    "open" | "opening" | "closing" | "closed"
  >(folded ? "closed" : "open");

  useLayoutEffect(() => {
    setFoldState((current) => {
      if (folded) {
        return current === "closed" || current === "closing"
          ? current
          : "closing";
      }
      return current === "open" || current === "opening" ? current : "opening";
    });
  }, [folded]);

  useEffect(() => {
    if (foldState !== "opening" && foldState !== "closing") return;
    // Hidden tabs and reduced-motion styles may never fire animationend.
    const timer = window.setTimeout(() => {
      setFoldState(folded ? "closed" : "open");
    }, 350);
    return () => window.clearTimeout(timer);
  }, [foldState, folded]);

  if (folded && foldState === "closed") return null;

  return (
    // `inert` keeps folded work out of tab order and off the screen reader.
    <div
      className="zen-fold-item"
      data-fold-state={foldState}
      inert={folded}
      onAnimationEnd={(event) => {
        if (event.target !== event.currentTarget) return;
        setFoldState(folded ? "closed" : "open");
      }}
    >
      {/* Keep padding off the Grid item itself. Otherwise its 4px bottom
       * padding survives a 0fr track and every folded row leaves a gap. */}
      <div>
        <div className="pb-1">
          {typeof children === "function" ? children() : children}
        </div>
      </div>
    </div>
  );
}

/** A turn item's identity, stable as the group it names grows. */
function turnItemKey(item: TurnItem): string {
  return item.type === "block" ? item.block.id : item.blocks[0].id;
}

/**
 * The line a turn's work folds behind: the agent's mark, and the clock —
 * ticking while the agent works, how long it took once it is done. Everything
 * the fold holds stays one click away, so the settled transcript reads as
 * prompt, answer, and a receipt for the work in between.
 */
function WorkFoldLine({
  title,
  kind,
  harness,
  agentMascot,
  live = false,
  expandable,
  open,
  onToggle,
}: {
  title: ReactNode;
  kind: ActivityPhaseKind;
  harness?: HarnessId;
  agentMascot?: Props["agentMascot"];
  live?: boolean;
  expandable: boolean;
  open: boolean;
  onToggle: () => void;
}) {
  const icon = (
    <span className="relative flex size-3.5 shrink-0 items-center justify-center">
      {open ? (
        // Open, the chevron stays put: it is the way back, and hunting for it
        // under the cursor is no way to close what you opened.
        <ChevronRight
          className="size-3.5 rotate-90 text-content/45"
          strokeWidth={1.75}
        />
      ) : (
        <>
          {agentMascot ? (
            <PixelMascot
              name={agentMascot.mascot}
              color={agentMascot.color}
              still
              className={`size-3.5 shrink-0 ${expandable ? "group-hover:opacity-0" : ""}`}
            />
          ) : harness ? (
            <HarnessIcon
              harness={harness}
              className={`size-3.5 shrink-0 ${expandable ? "group-hover:opacity-0" : ""}`}
            />
          ) : (
            <ActivityPhaseIcon
              kind={kind}
              className={expandable ? "group-hover:opacity-0" : ""}
            />
          )}
          {expandable ? (
            <ChevronRight
              className="absolute size-3.5 text-content/45 opacity-0 transition-opacity duration-200 group-hover:opacity-100"
              strokeWidth={1.75}
            />
          ) : null}
        </>
      )}
    </span>
  );
  // While the agent runs, the clock shimmers here rather than at the bottom,
  // which is now bare.
  const label = live ? (
    title
  ) : (
    <span className="min-w-0 flex-1 truncate font-sans text-sm text-content/50 transition-colors duration-200 group-hover:text-content/80">
      {title}
    </span>
  );
  // A settled Mono turn signs off in a pill, like the day separators.
  const content =
    agentMascot && !live ? (
      <MonoSignaturePill>
        {icon}
        {label}
      </MonoSignaturePill>
    ) : (
      <>
        {icon}
        {label}
      </>
    );
  const row = `flex w-full min-w-0 items-center gap-1.5 px-4 py-1 text-left${
    open ? " zen-fold-drop" : ""
  }`;

  if (!expandable) {
    return (
      <div
        className={`group ${row}`}
        role={live ? "status" : undefined}
        aria-live={live ? "polite" : undefined}
      >
        {content}
      </div>
    );
  }
  return (
    <button
      type="button"
      aria-expanded={open}
      aria-label={open ? "Hide the work" : "Show the work"}
      aria-live={live ? "polite" : undefined}
      onClick={onToggle}
      className={`group ${row}`}
    >
      {content}
    </button>
  );
}

function MonoSignaturePill({ children }: { children: ReactNode }) {
  return (
    <span className="flex min-w-0 items-center gap-1.5 rounded-full bg-content/[0.07] py-0.5 pr-3 pl-2">
      {children}
    </span>
  );
}

/**
 * The turn's work as phases. Related reasoning and calls stay together while
 * assistant prose remains outside as full-size transcript text. The phase the
 * agent is in stays open, with new steps scrolling inside a short window; the
 * moment it moves on the phase folds back to its header.
 */
type ActivityPhasesProps = {
  blocks: Block[];
  cwd?: string;
  done?: boolean;
  /** False inside a nested panel, which supplies its own gutter. */
  padded?: boolean;
  onApproval?: (requestId: number, decision: ApprovalDecision) => void;
  onOpenFile?: (path: string) => void;
  onOpenDiff?: (path: string) => void;
};

const ActivityPhases = memo(function ActivityPhases({
  blocks,
  cwd,
  done,
  padded = true,
  onApproval,
  onOpenFile,
  onOpenDiff,
}: ActivityPhasesProps) {
  const phases = useMemo(() => buildActivityPhases(blocks), [blocks]);

  return (
    <div className={`flex min-w-0 flex-col gap-1 ${padded ? "px-4" : ""}`}>
      {phases.map((phase, index) => (
        <ActivityPhaseGroup
          key={phase.id}
          phase={phase}
          cwd={cwd}
          active={!done && index === phases.length - 1}
          onApproval={onApproval}
          onOpenFile={onOpenFile}
          onOpenDiff={onOpenDiff}
        />
      ))}
    </div>
  );
}, sameActivity);

/**
 * A settled group is the same calls it was on the last token. Comparing the
 * blocks themselves keeps every earlier turn out of the streaming re-render,
 * which is most of what makes a long transcript stutter while the agent works.
 */
function sameActivity(a: ActivityPhasesProps, b: ActivityPhasesProps): boolean {
  return (
    a.cwd === b.cwd &&
    a.done === b.done &&
    a.padded === b.padded &&
    a.onApproval === b.onApproval &&
    a.onOpenFile === b.onOpenFile &&
    a.onOpenDiff === b.onOpenDiff &&
    a.blocks.length === b.blocks.length &&
    a.blocks.every((block, index) => block === b.blocks[index])
  );
}

/**
 * Hold the reader's place while turns above the viewport change height. The
 * scroller opts out of native scroll anchoring, so late markdown, image or
 * disclosure sizing above the viewport needs an explicit correction. Loaded
 * turns use their real heights; scrolling alone must not cause corrections.
 */
function useTurnScrollAnchor(
  el: HTMLDivElement | null,
  enabled: boolean,
  stickToBottom: RefObject<boolean>,
  onAdjust: (el: HTMLElement) => void,
) {
  useLayoutEffect(() => {
    const inner = el?.querySelector("[data-transcript-content]");
    if (!enabled || !el || !inner) return;
    const heights = new WeakMap<Element, number>();
    const resize = new ResizeObserver((entries) => {
      // A parked transcript's scroller is detached and measures zero.
      if (!el.isConnected) return;
      const viewportTop = el.getBoundingClientRect().top;
      let shift = 0;
      let precedingDelta = 0;
      const byTurn = new Map(entries.map((entry) => [entry.target, entry]));
      // Entries can arrive out of order. Later turns already include the
      // height corrections of earlier turns in their new layout position.
      for (const turn of inner.children) {
        const entry = byTurn.get(turn);
        if (!entry) continue;
        const height =
          entry.borderBoxSize?.[0]?.blockSize ?? entry.contentRect.height;
        const previous = heights.get(entry.target);
        heights.set(entry.target, height);
        if (previous === undefined || stickToBottom.current) continue;
        // Only turns that sat wholly above the view. A turn the reader is
        // looking at grows downward from where they are reading.
        const top = entry.target.getBoundingClientRect().top - precedingDelta;
        if (top + previous <= viewportTop) shift += height - previous;
        precedingDelta += height - previous;
      }
      if (shift) {
        el.scrollTop += shift;
        onAdjust(el);
      }
    });
    let observed = new WeakSet<Element>();
    const observeTurns = () => {
      for (const turn of inner.children) {
        if (observed.has(turn) || !turn.classList.contains("transcript-turn"))
          continue;
        observed.add(turn);
        resize.observe(turn);
      }
    };
    const mutations = new MutationObserver((records) => {
      // Removal is rare (a rewind or edit), so start over rather than hold
      // detached turns. Re-observed turns report the height already stored.
      if (records.some((record) => record.removedNodes.length > 0)) {
        resize.disconnect();
        observed = new WeakSet();
      }
      observeTurns();
    });
    mutations.observe(inner, { childList: true });
    observeTurns();
    return () => {
      mutations.disconnect();
      resize.disconnect();
    };
  }, [el, enabled, stickToBottom, onAdjust]);
}

/**
 * Keep a live phase body on its newest step. Pinning happens in layout
 * before paint so the window follows without a visible hitch; only a real
 * wheel away from the bottom pauses that.
 */
function useLivePhaseScroll(
  el: HTMLDivElement | null,
  enabled: boolean,
  steps: Block[],
) {
  const stickToBottom = useRef(true);
  const lastScrollTop = useRef(0);
  const wasEnabled = useRef(false);

  const pin = useCallback(() => {
    if (!el) return;
    stickToBottom.current = followsAfterScroll(
      el,
      lastScrollTop.current,
      stickToBottom.current,
    );
    if (stickToBottom.current) el.scrollTop = el.scrollHeight;
    lastScrollTop.current = el.scrollTop;
  }, [el]);

  useLayoutEffect(() => {
    if (!enabled) {
      wasEnabled.current = false;
      return;
    }
    if (!wasEnabled.current) {
      stickToBottom.current = true;
      lastScrollTop.current = el?.scrollTop ?? 0;
      wasEnabled.current = true;
    }
    pin();
  }, [el, enabled, pin, steps]);

  useEffect(() => {
    if (!el || !enabled) return;

    const onScroll = () => {
      stickToBottom.current = followsAfterScroll(
        el,
        lastScrollTop.current,
        stickToBottom.current,
      );
      lastScrollTop.current = el.scrollTop;
    };
    const onWheel = (e: WheelEvent) => {
      if (!nestedScrollAbsorbsWheel(el, e.deltaY)) return;
      if (e.deltaY < 0) stickToBottom.current = false;
      e.stopPropagation();
    };

    el.addEventListener("scroll", onScroll, { passive: true });
    el.addEventListener("wheel", onWheel, { passive: true });
    const inner = el.firstElementChild;
    const observer = new ResizeObserver(pin);
    if (inner) observer.observe(inner);
    pin();
    return () => {
      el.removeEventListener("scroll", onScroll);
      el.removeEventListener("wheel", onWheel);
      observer.disconnect();
    };
  }, [el, enabled, pin]);
}

/** The live ticker shares the identity line instead of adding a work row. */
function MonoTurnHeader({
  blocks,
  title,
  name,
  harness,
  agentMascot,
  active,
  live,
  waitingForAnswers,
  backgroundTasks,
  onShowWork,
  workExpanded,
  searchCurrent,
}: {
  blocks: Block[];
  title: ReactNode;
  name?: string;
  harness?: HarnessId;
  agentMascot?: Props["agentMascot"];
  active: boolean;
  live: boolean;
  waitingForAnswers: boolean;
  backgroundTasks?: string[];
  onShowWork?: () => void;
  workExpanded?: boolean;
  searchCurrent: boolean;
}) {
  const activity = useMemo(
    () => blocks.filter((block) => !block.internal && !block.draft),
    [blocks],
  );
  let status = useMemo(
    () => monoWorkStatus(activity, active),
    [activity, active],
  );
  if (active && waitingForAnswers && !activity.some(needsApproval)) {
    status = { ...status, key: "question", label: "Waiting for answers…" };
  } else if (active && backgroundTasks?.length && status.kind === "think") {
    status = {
      ...status,
      key: "background",
      label:
        backgroundTasks.length === 1
          ? "Waiting for background task…"
          : `Waiting for ${backgroundTasks.length} background tasks…`,
    };
  }
  const mark = agentMascot ? (
    <PixelMascot
      name={agentMascot.mascot}
      color={agentMascot.color}
      still
      className="size-3.5 shrink-0"
    />
  ) : harness ? (
    <HarnessIcon harness={harness} className="size-3.5 shrink-0" />
  ) : (
    <ActivityPhaseIcon kind={status.kind} />
  );
  const ticker = (
    <MonoWorkTicker status={{ ...status, active: live }} showIcon={false} />
  );
  return (
    <div
      data-mono-work
      data-transcript-search-item
      data-transcript-search-current={searchCurrent || undefined}
      className="flex min-w-0 items-center gap-1.5 px-4 py-1 font-sans text-sm text-content/50"
    >
      {active ? (
        <>
          {agentMascot ? (
            // The pill holds the Mono's signature from the first token on;
            // the ticker runs beside it.
            <>
              <span className="max-w-[45%] shrink-0">
                <MonoSignaturePill>
                  {mark}
                  {name ? (
                    <span className="min-w-0 truncate font-medium text-content/80">
                      {name}
                    </span>
                  ) : null}
                </MonoSignaturePill>
              </span>
              <span aria-hidden className="shrink-0 text-content/25">
                ·
              </span>
            </>
          ) : (
            <>
              {mark}
              {name ? (
                <>
                  <span className="max-w-[45%] truncate">{name}</span>
                  <span aria-hidden className="shrink-0 text-content/25">
                    ·
                  </span>
                </>
              ) : null}
            </>
          )}
          {onShowWork ? (
            <button
              type="button"
              title={workExpanded ? "Hide activity" : "Show activity"}
              aria-label={workExpanded ? "Hide activity" : "Show activity"}
              aria-expanded={!!workExpanded}
              onClick={onShowWork}
              className="min-w-0 flex-1 cursor-pointer text-left outline-none transition-opacity duration-150 hover:opacity-70 focus-visible:opacity-70"
            >
              {ticker}
            </button>
          ) : (
            <div className="min-w-0 flex-1">{ticker}</div>
          )}
        </>
      ) : agentMascot ? (
        <MonoSignaturePill>
          {mark}
          <span className="min-w-0 truncate">{title}</span>
        </MonoSignaturePill>
      ) : (
        <>
          {mark}
          <span className="min-w-0 truncate">{title}</span>
        </>
      )}
    </div>
  );
}

type MonoWorkApprovalsProps = Pick<
  ActivityPhasesProps,
  "blocks" | "cwd" | "onApproval" | "onOpenFile" | "onOpenDiff"
>;

/** Pending approvals stay actionable while the rest of the work is hidden. */
const MonoWorkApprovals = memo(function MonoWorkApprovals({
  blocks,
  cwd,
  onApproval,
  onOpenFile,
  onOpenDiff,
}: MonoWorkApprovalsProps) {
  return (
    <div className="flex min-w-0 flex-col px-4">
      {blocks.filter(needsApproval).map((block) => (
        <ToolCall
          key={block.id}
          block={block}
          cwd={cwd}
          embedded
          onApproval={onApproval}
          onOpenFile={onOpenFile}
          onOpenDiff={onOpenDiff}
        />
      ))}
    </div>
  );
}, sameActivity);

/** Full turn activity in transcript order, without the chat's work folding. */
export function MonoActivityTrail({
  blocks,
  live = false,
  cwd,
  onApproval,
  onOpenFile,
  onOpenDiff,
}: Omit<ActivityPhasesProps, "done" | "padded"> & { live?: boolean }) {
  const steps = blocks.filter(
    (block) =>
      !block.internal &&
      !block.draft &&
      block.role !== "user" &&
      block.role !== "handoff" &&
      (block.text.trim() ||
        block.tool ||
        block.image ||
        block.taskList ||
        block.plan),
  );
  // What it said reads as text; the work between hangs on a short timeline.
  const segments: { prose: boolean; blocks: Block[] }[] = [];
  for (const block of steps) {
    const prose = isProseBlock(block);
    const last = segments[segments.length - 1];
    if (last && !prose && !last.prose) last.blocks.push(block);
    else segments.push({ prose, blocks: [block] });
  }
  return (
    <ol
      aria-label="Activity in chronological order"
      className="flex min-w-0 flex-col gap-3"
    >
      {segments.map((segment) => {
        const first = segment.blocks[0];
        if (segment.prose)
          return (
            <li
              key={first.id}
              data-mono-activity-block={first.id}
              className="min-w-0 px-1 text-content/80"
            >
              <AgentMarkdown
                text={first.text}
                cwd={cwd}
                onOpenFile={onOpenFile}
                className="mono-run-report"
              />
            </li>
          );
        return (
          <li
            key={first.id}
            className="mono-activity-steps flex min-w-0 flex-col px-1"
          >
            {segment.blocks.map((block, step) => (
              <div
                key={block.id}
                data-mono-activity-block={block.id}
                className="relative min-w-0 pl-4"
              >
                {/* The rail runs between the steps' dots, not past the ends. */}
                <span
                  aria-hidden="true"
                  className={`absolute left-[2.5px] w-px bg-content/12 ${
                    step === 0 ? "top-[13px]" : "top-0"
                  } ${
                    step === segment.blocks.length - 1 ? "h-[13px]" : "bottom-0"
                  } ${segment.blocks.length === 1 ? "hidden" : ""}`}
                />
                <span
                  aria-hidden="true"
                  className="absolute left-0 top-[10px] size-1.5 rounded-full bg-content/25"
                />
                {block.role === "plan" ||
                block.role === "tasks" ||
                block.role === "image" ? (
                  <TranscriptBlock
                    block={block}
                    layout="full"
                    stickyIndex={steps.indexOf(block)}
                    embedded
                    cwd={cwd}
                  />
                ) : (
                  <ActivityRow
                    block={block}
                    live={live}
                    cwd={cwd}
                    onApproval={onApproval}
                    onOpenFile={onOpenFile}
                    onOpenDiff={onOpenDiff}
                  />
                )}
              </div>
            ))}
          </li>
        );
      })}
    </ol>
  );
}

/**
 * One phase: a header the whole group hangs off, and the steps under it on a
 * rail. Folding is automatic — the group opens while it is the live one and
 * closes when the agent moves on — until you click, after which it stays where
 * you put it. A step still waiting on you keeps the group open regardless.
 * While live, the open body stays a short scrolling window pinned to the
 * newest step; after the turn settles an opened group is full height again.
 */
function ActivityPhaseGroup({
  phase,
  cwd,
  active,
  onApproval,
  onOpenFile,
  onOpenDiff,
}: {
  phase: ActivityPhase;
  cwd?: string;
  active: boolean;
  onApproval?: (requestId: number, decision: ApprovalDecision) => void;
  onOpenFile?: (path: string) => void;
  onOpenDiff?: (path: string) => void;
}) {
  const [override, setOverride] = useState<boolean | null>(null);
  const waiting = phase.steps.some(needsApproval);
  const open = waiting || (override ?? active);
  const [liveScroller, setLiveScroller] = useState<HTMLDivElement | null>(null);
  useLivePhaseScroll(liveScroller, active && open, phase.steps);
  // Steps already here when the group mounted, or that landed while it was
  // folded, are history: only a step you watch arrive gets the entrance.
  const settled = useRef<Set<Block["id"]> | null>(null);
  settled.current ??= new Set(phase.steps.map((step) => step.id));
  useEffect(() => {
    for (const step of phase.steps) settled.current?.add(step.id);
  }, [phase.steps]);
  const turnFor = useStepQueue();
  const title = activityPhaseTitle(phase, active);
  const monoCodePhase = !!monoCodeWorkSummary(phase.steps, active);
  // Opening a group on purpose is also how you read the line that titled it,
  // whole. The auto-open while it runs is a live view, not a reading one, and
  // a one-line note the header already shows in full has nothing to add.
  const headline =
    override === true && phase.headline && headlineHasMore(phase.headline)
      ? phase.headline
      : undefined;
  const inert = phase.steps.length === 0 && !headlineHasMore(phase.headline);

  // A lone call the agent never introduced is not a group: a header repeating
  // the single row under it says nothing twice.
  if (!phase.headline && phase.steps.length === 1) {
    return (
      <div className="flex min-w-0 items-start gap-1.5">
        {monoCodePhase ? null : (
          <ActivityPhaseIcon kind={phase.kind} className="mt-[7px]" />
        )}
        <div className="min-w-0 flex-1">
          <ActivityRow
            block={phase.steps[0]}
            cwd={cwd}
            live={active}
            onApproval={onApproval}
            onOpenFile={onOpenFile}
            onOpenDiff={onOpenDiff}
          />
        </div>
      </div>
    );
  }

  const label = active ? (
    <Shimmer className="min-w-0 truncate font-sans text-sm" duration={1.6}>
      {title}
    </Shimmer>
  ) : (
    // Dimmed to sit with the icons: the work is chrome around the answer, and
    // only the answer reads at full strength.
    <span className="min-w-0 flex-1 truncate font-sans text-sm text-content/50 transition-colors duration-200 group-hover:text-content/80">
      {title}
    </span>
  );

  // A line the agent wrote with nothing under it is just that line.
  if (inert) {
    return (
      <div className="flex min-w-0 items-center gap-1.5 py-1">
        <ActivityPhaseIcon kind={phase.kind} />
        {label}
      </div>
    );
  }

  return (
    <div className="flex min-w-0 flex-col">
      <button
        type="button"
        aria-expanded={open}
        aria-label={
          open ? `Hide the steps for ${title}` : `Show the steps for ${title}`
        }
        onClick={() => setOverride(!open)}
        className="group flex w-full min-w-0 items-center gap-1.5 py-1 text-left"
      >
        {/*
         * The two icons share one 14px box, so the swap is instant: fading
         * between them leaves both half-drawn on top of each other.
         */}
        <span className="relative flex size-3.5 shrink-0 items-center justify-center">
          {monoCodePhase ? (
            <MonoCodeMark className="size-3.5 group-hover:opacity-0" />
          ) : (
            <ActivityPhaseIcon
              kind={phase.kind}
              className="group-hover:opacity-0"
            />
          )}
          <ChevronRight
            className={`absolute size-3.5 text-content/45 opacity-0 transition-transform duration-200 group-hover:opacity-100 ${
              open ? "rotate-90" : ""
            }`}
            strokeWidth={1.75}
          />
        </span>
        {label}
      </button>
      <div className="zen-phase-body" data-open={open}>
        {open ? (
          <div
            ref={setLiveScroller}
            className={active || !open ? "zen-phase-live" : undefined}
          >
            <div className="flex min-w-0 flex-col">
              {headline ? (
                <div className="zen-phase-step py-1">
                  <AgentMarkdown
                    className={
                      headline.role === "reasoning"
                        ? "agent-reasoning"
                        : undefined
                    }
                    text={headline.text}
                    cwd={cwd}
                    onOpenFile={onOpenFile}
                  />
                </div>
              ) : null}
              {phase.steps.map((block) => {
                const arriving = active && !settled.current?.has(block.id);
                return (
                  <PhaseStep
                    key={block.id}
                    live={active}
                    turn={arriving ? turnFor(block.id) : undefined}
                  >
                    <ActivityRow
                      block={block}
                      cwd={cwd}
                      live={active}
                      onApproval={onApproval}
                      onOpenFile={onOpenFile}
                      onOpenDiff={onOpenDiff}
                    />
                  </PhaseStep>
                );
              })}
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}

type StepTurn = { wait: number; pace: number };

/**
 * A group's queue of arriving steps: how long each one waits for the step
 * before it to finish, and how long its own entrance then takes. A step keeps
 * the turn it was first given however often the group renders.
 */
function useStepQueue() {
  const queue = useRef({ next: 0, turns: new Map<Block["id"], StepTurn>() });

  return (id: Block["id"]) => {
    const { turns } = queue.current;
    let turn = turns.get(id);
    if (!turn) {
      const now = performance.now();
      const start = Math.max(now, queue.current.next);
      const wait = start - now;
      const backlog =
        (STEP_QUEUE_MS - wait) / (STEP_QUEUE_MS - STEP_QUEUE_CALM_MS);
      const pace = Math.max(
        STEP_ENTRANCE_MIN_MS,
        STEP_ENTRANCE_MS * Math.min(1, backlog),
      );
      queue.current.next = start + pace;
      turn = { wait, pace };
      turns.set(id, turn);
    }
    return turn;
  };
}

/**
 * One step on a phase's rail. A step that lands while you watch makes room
 * first — what is below glides down, the rail runs into the gap and branches
 * off — and only then does the row fade in. One that lands behind others
 * stays out of the layout until its turn. The grid and clipping that does
 * that come off once the row has settled, so nothing inside stays clipped.
 */
function PhaseStep({
  live,
  turn: arrival,
  children,
}: {
  live: boolean;
  /** Set only on the render a step arrives in; later renders drop it. */
  turn?: StepTurn;
  children: ReactNode;
}) {
  const [turn] = useState(arrival);
  const [stage, setStage] = useState<"waiting" | "entering" | "settled">(() =>
    !turn ? "settled" : turn.wait > 0 ? "waiting" : "entering",
  );

  useEffect(() => {
    if (stage !== "waiting" || !turn) return;
    const timer = window.setTimeout(() => setStage("entering"), turn.wait);
    return () => window.clearTimeout(timer);
  }, [stage, turn]);

  return (
    <div
      className="zen-phase-step"
      style={
        turn
          ? ({ "--step-ms": `${Math.round(turn.pace)}ms` } as CSSProperties)
          : undefined
      }
      data-live={live || undefined}
      data-waiting={stage === "waiting" || undefined}
      data-entering={stage === "entering" || undefined}
      onAnimationEnd={(e) => {
        // The row's own fade is the last beat; nested rails bubble theirs.
        if (
          e.animationName === "zen-step-in" &&
          (e.target as Element).parentElement === e.currentTarget
        ) {
          setStage("settled");
        }
      }}
    >
      {children}
    </div>
  );
}

/**
 * Every delegated run in the turn, one row each. The main transcript cycles —
 * work folds behind a line, prose replaces prose — and a subagent you are
 * watching must not move while that happens, so these rows are never part of a
 * fold and hold their place from the moment the agents start.
 *
 * A row is a mascot, a name, and what that agent is doing right now. Click it
 * and the agent's own trail opens underneath.
 */
function SubagentStack({
  blocks,
  cwd,
  live = false,
  embedded = false,
  onOpenFile,
  onOpenDiff,
}: {
  blocks: Block[];
  cwd?: string;
  live?: boolean;
  embedded?: boolean;
  onOpenFile?: (path: string) => void;
  onOpenDiff?: (path: string) => void;
}) {
  return (
    <div className={`flex min-w-0 flex-col ${embedded ? "" : "px-4"}`}>
      {blocks.map((block) => (
        <SubagentRow
          key={block.id}
          block={block}
          cwd={cwd}
          live={live}
          onOpenFile={onOpenFile}
          onOpenDiff={onOpenDiff}
        />
      ))}
    </div>
  );
}

/**
 * One delegated run's row, stateful about being opened. A run that died opens
 * itself, so the provider's reason is not buried behind a face that looks like
 * every other finished one. A click takes the row over from there and it stays
 * where the reader puts it. The same row serves inside a settled turn's trail,
 * where the run sits as one step of the work it was spawned from.
 */
function SubagentRow({
  block,
  cwd,
  live = false,
  onOpenFile,
  onOpenDiff,
}: {
  block: Block;
  cwd?: string;
  live?: boolean;
  onOpenFile?: (path: string) => void;
  onOpenDiff?: (path: string) => void;
}) {
  const [override, setOverride] = useState<boolean | null>(null);
  const open = override ?? toolCallState(block) === "rejected";
  return (
    <SubagentPanel
      block={block}
      cwd={cwd}
      live={live}
      open={open}
      onToggle={() => setOverride(!open)}
      onOpenFile={onOpenFile}
      onOpenDiff={onOpenDiff}
    />
  );
}

/**
 * One delegated run: its name, what it is doing, and — once opened — the trail
 * it left, with the same tool rows, thinking and prose the main transcript
 * shows. While it runs the trail is a short window pinned to the newest step,
 * so a subagent doing hundreds of things cannot push the turn off the screen.
 */
function SubagentPanel({
  block,
  cwd,
  live = false,
  open,
  onToggle,
  onOpenFile,
  onOpenDiff,
}: {
  block: Block;
  cwd?: string;
  live?: boolean;
  open: boolean;
  onToggle: () => void;
  onOpenFile?: (path: string) => void;
  onOpenDiff?: (path: string) => void;
}) {
  const name = subagentName(block);
  const brief = subagentBrief(block);
  const model = subagentModelName(block);
  const state = toolCallState(block);
  const active = live && state === "pending";
  const steps = block.agentRun?.steps ?? [];
  // The run's trail as transcript blocks, so the panel groups it the way the
  // main transcript groups the agent's own work: what it said, then the calls
  // that line introduced, folding behind it once it moves on.
  const stepBlocks = useMemo(() => steps.map(agentStepBlock), [steps]);
  const status = subagentStatusLine(block, steps);
  const report = subagentReport(block);
  const failed = state === "rejected";

  // The name takes the room it needs and gives the rest back: a provider that
  // names a run with its whole brief must not push the row off the pane.
  const label = (
    <span className="flex min-w-0 flex-1 items-baseline gap-2">
      {active ? (
        <Shimmer
          className="min-w-0 flex-1 truncate font-sans text-sm"
          duration={1.6}
        >
          {name}
        </Shimmer>
      ) : (
        <span
          className={`min-w-0 flex-1 truncate font-sans text-sm transition-colors duration-200 ${
            state === "rejected"
              ? "text-red-400"
              : "text-content/75 group-hover:text-content"
          }`}
        >
          {name}
        </span>
      )}
      {model || status ? (
        <span className="flex min-w-0 max-w-[55%] shrink-0 items-baseline gap-2 font-sans text-[12px] text-content/40">
          {model ? (
            <span className="truncate" title={`Model: ${model}`}>
              {model}
            </span>
          ) : null}
          {status ? <span className="shrink-0">{status}</span> : null}
        </span>
      ) : null}
    </span>
  );

  // A run that has not reported a step yet has nothing to open into. The row
  // still holds its place, so the chevron arriving does not move anything.
  if (steps.length === 0 && !report) {
    return (
      <div
        aria-label={`Subagent: ${name}`}
        title={brief}
        className="-mx-1.5 flex min-w-0 items-center gap-2 px-1.5 py-1"
      >
        <SubagentMascot name={name} state={state} active={active} />
        {label}
        <span className="size-3.5 shrink-0" />
      </div>
    );
  }

  return (
    <div className="flex min-w-0 flex-col">
      <button
        type="button"
        aria-expanded={open}
        aria-label={open ? `Hide ${name}'s work` : `Show ${name}'s work`}
        title={brief}
        onClick={onToggle}
        // An open row keeps the wash it lit up under the cursor, so the panel
        // below reads as hanging off it rather than off the transcript.
        className={`group -mx-1.5 flex w-full min-w-0 items-center gap-2 rounded-md px-1.5 py-1 text-left transition-colors duration-200 hover:bg-content/8 ${
          open ? "bg-content/8" : ""
        }`}
      >
        <SubagentMascot name={name} state={state} active={active} />
        {label}
        <ChevronRight
          className={`size-3.5 shrink-0 text-content/35 transition-transform duration-200 group-hover:text-content/60 ${
            open ? "rotate-90" : ""
          }`}
          strokeWidth={1.75}
        />
      </button>
      <div className="zen-phase-body" data-open={open}>
        {open ? (
          /*
           * No scroll window of its own. Each phase inside already keeps the
           * group the run is working in to a short pinned window; wrapping a
           * second window around them nests one 17.5rem scroller inside
           * another, and the inner one can never reach its own last row.
           */
          <div className="flex min-w-0 flex-col pb-1">
            <ActivityPhases
              blocks={stepBlocks}
              cwd={cwd}
              done={!active}
              padded={false}
              onOpenFile={onOpenFile}
              onOpenDiff={onOpenDiff}
            />
            {report ? (
              <div className="zen-phase-step py-1">
                {failed ? (
                  <pre className="min-w-0 whitespace-pre-wrap break-words font-mono text-[12px] leading-5 text-red-400/80">
                    {report}
                  </pre>
                ) : (
                  <AgentMarkdown
                    text={report}
                    cwd={cwd}
                    onOpenFile={onOpenFile}
                  />
                )}
              </div>
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  );
}

/**
 * The pixel mascot standing in for a subagent, hopping while it works. The
 * sprite is hashed off the agent's name, so the same reviewer keeps the same
 * face across a session and two agents in a row are told apart at a glance.
 */
function SubagentMascot({
  name,
  state,
  active = false,
}: {
  name: string;
  state: ToolCallState;
  active?: boolean;
}) {
  return (
    <ProjectMascot
      project={name}
      active={active}
      className={`size-3.5 shrink-0 ${
        state === "rejected"
          ? "text-red-400"
          : state === "pending"
            ? "text-content/70"
            : "text-content/45"
      }`}
    />
  );
}

/**
 * A mirrored step as the transcript block it stands for, so a subagent's trail
 * goes through the same rows — labels, file chips, diffs — as the main agent's.
 */
function agentStepBlock(step: AgentStep): Block {
  if (step.kind !== "tool") {
    return {
      id: step.id,
      role: step.kind === "reasoning" ? "reasoning" : "assistant",
      text: step.text,
    };
  }
  return {
    id: step.id,
    role: "tool",
    text: step.text,
    tool: {
      callId: step.id,
      title: step.text,
      ...(step.toolKind ? { kind: step.toolKind } : {}),
      ...(step.status ? { status: step.status } : {}),
      ...(step.detail ? { detail: step.detail } : {}),
      ...(step.preview ? { preview: step.preview } : {}),
    },
  };
}

/** What a delegated run is up to: its newest step, or how much it got through. */
/**
 * A run is counted, never narrated. Echoing the call in flight put a second
 * scrolling command line on every row — the shimmer on the name already says
 * the agent is working, and the count says how far it has got.
 */
function subagentStatusLine(block: Block, steps: AgentStep[]): string {
  if (toolCallState(block) === "rejected") return "failed";
  const tools = steps.filter((step) => step.kind === "tool").length;
  if (tools === 0) return "";
  const count = tools === 1 ? "1 step" : `${tools} steps`;
  // A step that failed inside a run that went on to finish still has to say so
  // here, or the row reads clean until someone opens the trail.
  const failed = steps.filter(
    (step) => step.kind === "tool" && isFailedStatus(step.status),
  ).length;
  if (!failed) return count;
  return `${count}, ${failed === 1 ? "1 failed" : `${failed} failed`}`;
}

/** Whether the line that titled a group has more in it than the header shows. */
function headlineHasMore(block?: Block): boolean {
  if (!block) return false;
  return block.role === "reasoning" || /\n\s*\n/.test(block.text.trim());
}

/** What the group was for, at a glance: look, change, run, think. */
function ActivityPhaseIcon({
  kind,
  className = "",
}: {
  kind: ActivityPhaseKind;
  className?: string;
}) {
  const props = {
    className: `size-3.5 shrink-0 text-content/45 ${className}`,
    strokeWidth: 1.75,
  };
  if (kind === "edit") return <PenLine {...props} />;
  if (kind === "research") return <Search {...props} />;
  if (kind === "run") return <Terminal {...props} />;
  if (kind === "agent") return <Bot {...props} />;
  if (kind === "think") return null;
  if (kind === "other") return <Wrench {...props} />;
  return <Minus {...props} />;
}

/**
 * One step of the agent's work, whatever that step was: a tool call, a thought,
 * a paragraph. In a phase the rail draws the bullet, so the row drops its own
 * leading icon and leans on the rail instead.
 */
function ActivityRow({
  block,
  cwd,
  live = false,
  onApproval,
  onOpenFile,
  onOpenDiff,
}: {
  block: Block;
  cwd?: string;
  live?: boolean;
  onApproval?: (requestId: number, decision: ApprovalDecision) => void;
  onOpenFile?: (path: string) => void;
  onOpenDiff?: (path: string) => void;
}) {
  if (isThinkingBlock(block)) {
    return (
      <ActivityThinkingRow
        block={block}
        cwd={cwd}
        expandable
        bare
        onOpenFile={onOpenFile}
      />
    );
  }
  if (block.interjection) {
    return <ActivityInterjectionRow block={block} />;
  }
  if (block.role === "system") {
    return <ActivityStatusRow block={block} />;
  }
  if (isProseBlock(block)) {
    return (
      <ActivityNoteRow
        block={block}
        cwd={cwd}
        bare
        expandable
        onOpenFile={onOpenFile}
      />
    );
  }
  // Only a settled turn routes a delegated run here; live turns pin the row
  // outside the trail. Either way it is the same row, so it still opens onto
  // the agent's own work.
  if (isSubagentBlock(block)) {
    return (
      <SubagentRow
        block={block}
        cwd={cwd}
        live={live}
        onOpenFile={onOpenFile}
        onOpenDiff={onOpenDiff}
      />
    );
  }
  return (
    <ActivityToolRow
      block={block}
      cwd={cwd}
      live={live}
      bare
      onApproval={onApproval}
      onOpenFile={onOpenFile}
      onOpenDiff={onOpenDiff}
    />
  );
}

/** A status row folded into the trail: one muted line, nothing to open. */
function ActivityStatusRow({ block }: { block: Block }) {
  return (
    <div className="flex min-w-0 items-center gap-1.5 py-1">
      <span
        title={block.text}
        className="min-w-0 flex-1 truncate font-sans text-sm text-content/50"
      >
        {block.text.trim()}
      </span>
    </div>
  );
}

/**
 * An interjection inside the work trail: one compact line naming where it came
 * from and what it said. It opens on a click, so folding the work never costs
 * you a note you wanted to read.
 */
function ActivityInterjectionRow({ block }: { block: Block }) {
  const [open, setOpen] = useState(false);
  const meta = block.interjection;
  if (!meta) return null;
  const chrome = interjectionChrome(meta);
  const summary = proseSummary(block.text);
  const label = (
    <span className="min-w-0 flex-1 truncate font-sans text-sm">
      <span className="text-content/55">{chrome.label}</span>
      {chrome.severityText ? (
        <span className={`text-[11px] ${chrome.severityClass}`}>
          {" "}
          {chrome.severityText}
        </span>
      ) : null}
      {summary ? (
        <span className="text-content/50 transition-colors duration-200 group-hover:text-content/75">
          {" · "}
          {summary}
        </span>
      ) : null}
    </span>
  );

  if (!block.text.trim()) {
    return (
      <div
        aria-label={`${chrome.label} note`}
        className="flex min-w-0 items-center gap-1.5 py-1"
      >
        {label}
      </div>
    );
  }

  return (
    <div className="flex min-w-0 flex-col">
      <button
        type="button"
        aria-expanded={open}
        aria-label={
          open ? `Hide the ${chrome.label} note` : `${chrome.label}: ${summary}`
        }
        onClick={() => setOpen((value) => !value)}
        className="group flex min-w-0 items-center gap-1.5 py-1 text-left"
      >
        {label}
      </button>
      {open ? (
        <div className="min-w-0 pb-2">
          <pre className={INTERJECTION_BODY}>{block.text}</pre>
        </div>
      ) : null}
    </div>
  );
}

/**
 * The line that keeps a long think from reading as a stall. Opening the fold
 * around it does not open the thought itself — reasoning is only ever read on
 * purpose, one line until you ask for it.
 */
function ActivityThinkingRow({
  block,
  cwd,
  expandable = false,
  bare = false,
  onOpenFile,
}: {
  block: Block;
  cwd?: string;
  expandable?: boolean;
  bare?: boolean;
  onOpenFile?: (path: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const text = proseSummary(block.text) || "Thinking";
  // In a group the rail is the bullet, so there is nothing to breathe while
  // reasoning streams in — the line itself does.
  const pulse = block.streaming ? "zen-thinking-pulse" : "";
  const icon = bare ? null : (
    <Minus
      className={`size-3.5 shrink-0 text-content/40 ${pulse}`}
      strokeWidth={1.75}
    />
  );
  const label = (
    <span
      className={`min-w-0 flex-1 truncate font-sans text-sm text-content/50 ${
        bare ? pulse : ""
      }`}
    >
      {text}
    </span>
  );

  if (!expandable) {
    return (
      <div
        aria-label={`Thinking: ${text}`}
        className="flex min-w-0 items-center gap-1.5 py-1"
      >
        {icon}
        {label}
      </div>
    );
  }

  return (
    <div className="flex min-w-0 flex-col">
      <button
        type="button"
        aria-expanded={open}
        aria-label={open ? "Hide thinking" : `Show thinking: ${text}`}
        onClick={() => setOpen((value) => !value)}
        className="group flex min-w-0 items-center gap-1.5 py-1 text-left"
      >
        {icon}
        <span
          className={`min-w-0 flex-1 truncate font-sans text-sm text-content/50 transition-colors duration-200 group-hover:text-content/75 ${
            bare ? pulse : ""
          }`}
        >
          {text}
        </span>
      </button>
      {open ? (
        <div className={`min-w-0 pb-2 ${bare ? "" : "pl-5"}`}>
          <AgentMarkdown
            className="agent-reasoning"
            text={block.text}
            cwd={cwd}
            onOpenFile={onOpenFile}
          />
        </div>
      ) : null}
    </div>
  );
}

/**
 * A line the agent wrote mid-run, kept to one line. It opens on click, so
 * folding the work never costs you a paragraph you wanted to read.
 */
function ActivityNoteRow({
  block,
  cwd,
  bare = false,
  expandable = false,
  onOpenFile,
}: {
  block: Block;
  cwd?: string;
  bare?: boolean;
  expandable?: boolean;
  onOpenFile?: (path: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const text = proseSummary(block.text);
  const icon = bare ? null : (
    <Minus className="size-3.5 shrink-0 text-content/50" strokeWidth={1.75} />
  );

  if (!expandable) {
    return (
      <div
        aria-label={`Agent said: ${text}`}
        className="flex min-w-0 items-center gap-1.5 py-1"
      >
        {icon}
        <span className="min-w-0 flex-1 truncate font-sans text-sm text-content/70">
          {text}
        </span>
      </div>
    );
  }

  return (
    <div className="flex min-w-0 flex-col">
      <button
        type="button"
        aria-expanded={open}
        aria-label={open ? "Hide the full note" : `Agent said: ${text}`}
        onClick={() => setOpen((value) => !value)}
        className="group flex min-w-0 items-center gap-1.5 py-1 text-left"
      >
        {icon}
        <span className="min-w-0 flex-1 truncate font-sans text-sm text-content/70 transition-colors duration-200 group-hover:text-content">
          {text}
        </span>
      </button>
      {open ? (
        <div className="min-w-0 pb-2">
          <AgentMarkdown text={block.text} cwd={cwd} onOpenFile={onOpenFile} />
        </div>
      ) : null}
    </div>
  );
}

function ActivityToolRow({
  block,
  cwd,
  live = false,
  bare = false,
  onApproval,
  onOpenFile,
  onOpenDiff,
}: {
  block: Block;
  cwd?: string;
  live?: boolean;
  bare?: boolean;
  onApproval?: (requestId: number, decision: ApprovalDecision) => void;
  onOpenFile?: (path: string) => void;
  onOpenDiff?: (path: string) => void;
}) {
  const [errorOpen, setErrorOpen] = useState(false);
  const appCall = monoCodeToolCall(block);
  if (appCall) {
    return (
      <MonoCodeCallRow block={block} call={appCall} onApproval={onApproval} />
    );
  }
  const label = toolCallLabel(block, cwd);
  const state = toolCallState(block);
  const pending = needsApproval(block);
  const errorDetail =
    !pending && state === "rejected" ? block.tool?.detail?.trim() : undefined;
  const summary = (
    <ToolCallSummary
      label={label}
      preview={block.tool?.preview}
      cwd={cwd}
      chip={bare}
      failed={state === "rejected"}
      status={state}
      onOpenFile={onOpenFile}
      onOpenDiff={onOpenDiff}
    />
  );

  return (
    <div className="flex min-w-0 flex-col">
      {errorDetail ? (
        <div
          aria-label={`Failed tool call: ${label}`}
          className="group flex min-w-0 items-center gap-1.5 py-1"
        >
          {bare ? null : <ActivityToolIcon state={state} live={live} />}
          <div
            className="flex min-w-0 flex-1 cursor-pointer"
            onClick={() => setErrorOpen((value) => !value)}
          >
            {summary}
          </div>
          <ToolCallStatusIcon state={state} />
          <button
            type="button"
            aria-expanded={errorOpen}
            aria-label={`${errorOpen ? "Hide" : "Show"} error details for ${label}`}
            onClick={() => setErrorOpen((value) => !value)}
            className="-m-1 shrink-0 rounded p-1"
          >
            <ChevronRight
              className={`size-3.5 text-red-400/60 transition-transform ${errorOpen ? "rotate-90" : ""}`}
              strokeWidth={1.75}
            />
          </button>
        </div>
      ) : (
        <div
          aria-label={`Tool call: ${label}`}
          className="flex min-w-0 items-center gap-1.5 py-1"
        >
          {bare ? null : <ActivityToolIcon state={state} live={live} />}
          {summary}
          {pending ? null : <ToolCallStatusIcon state={state} />}
        </div>
      )}
      {pending ? (
        <ApprovalControls block={block} onApproval={onApproval} />
      ) : null}
      {errorOpen && errorDetail ? (
        <pre
          className={`min-w-0 whitespace-pre-wrap break-words py-1 font-mono text-[12px] leading-5 text-red-400/80 ${bare ? "" : "pl-5"}`}
        >
          {errorDetail}
        </pre>
      ) : null}
    </div>
  );
}

function MonoCodeMark({ className = "size-4" }: { className?: string }) {
  return <img src="/monocode.png" alt="" className={`shrink-0 ${className}`} />;
}

/** MonoCode commands read like the other activity rows; failures expose their output. */
function MonoCodeCallRow({
  block,
  call,
  onApproval,
}: {
  block: Block;
  call: MonoCodeToolCall;
  onApproval?: (requestId: number, decision: ApprovalDecision) => void;
}) {
  const state = toolCallState(block);
  const output =
    block.tool?.detail?.trim() || block.tool?.preview?.output?.trim();
  const [errorOpen, setErrorOpen] = useState(false);
  const hasError = state === "rejected" && !!output;
  const pendingApproval = needsApproval(block);
  const command = `monocode app ${call.action}`;
  const verb = pendingApproval
    ? "Run"
    : state === "pending"
      ? "Running"
      : "Ran";
  const summary = (
    <>
      <span
        className={`shrink-0 font-sans text-sm ${state === "rejected" ? "text-red-400" : "text-content/50"}`}
      >
        {verb}
      </span>
      <span
        className={`flex min-w-0 max-w-full items-center gap-1 rounded bg-content/6 px-1 font-mono text-[13px] ${state === "rejected" ? "text-red-400" : "text-content/70"}`}
        title={command}
      >
        <MonoCodeMark className="size-3.5" />
        <span className="min-w-0 truncate">{command}</span>
      </span>
      <ToolCallStatusIcon state={state} />
      {hasError ? (
        <ChevronRight
          className={`size-3.5 shrink-0 text-red-400/60 transition-transform ${errorOpen ? "rotate-90" : ""}`}
          strokeWidth={1.75}
        />
      ) : null}
    </>
  );
  return (
    <div data-monocode-tool-call={call.action} className="min-w-0">
      {hasError ? (
        <button
          type="button"
          aria-expanded={errorOpen}
          aria-label={`${errorOpen ? "Hide" : "Show"} error details for MonoCode: ${call.label}`}
          onClick={() => setErrorOpen((value) => !value)}
          className="flex w-full min-w-0 items-center gap-1.5 py-1 text-left"
        >
          {summary}
        </button>
      ) : (
        <div className="flex min-w-0 items-center gap-1.5 py-1">{summary}</div>
      )}
      {errorOpen && hasError ? (
        <pre className="min-w-0 whitespace-pre-wrap break-words py-1 pl-5 font-mono text-[12px] leading-5 text-red-400/80">
          {output}
        </pre>
      ) : null}
      {pendingApproval ? (
        <pre className="max-h-32 min-w-0 overflow-auto whitespace-pre-wrap break-all py-1 pl-5 font-mono text-[12px] leading-5 text-content/70">
          {call.command}
        </pre>
      ) : null}
      <ApprovalControls block={block} onApproval={onApproval} />
    </div>
  );
}

function ActivityToolIcon({
  state,
  live = false,
}: {
  state: ToolCallState;
  live?: boolean;
}) {
  if (state === "pending") {
    return (
      <CircleDashed
        className={`size-3.5 shrink-0 text-content/40 ${live ? "zen-tool-spin" : ""}`}
        strokeWidth={1.75}
      />
    );
  }

  return (
    <Minus className="size-3.5 shrink-0 text-content/50" strokeWidth={1.75} />
  );
}

/** Failure stays marked. Running and success do not get a trailing icon. */
function ToolCallStatusIcon({ state }: { state: ToolCallState }) {
  if (state === "rejected") {
    return <X className="size-3.5 shrink-0 text-red-400" strokeWidth={2} />;
  }
  return null;
}

function useElapsedFrom(
  startedAt: number | undefined,
  paused: boolean,
): number | null {
  const fallback = useRef<number | null>(null);
  const pausedMs = useRef(0);
  const pauseStarted = useRef<number | null>(null);
  const seenStartedAt = useRef(startedAt);

  if (seenStartedAt.current !== startedAt) {
    seenStartedAt.current = startedAt;
    fallback.current = null;
    pausedMs.current = 0;
    pauseStarted.current = paused ? Date.now() : null;
  }

  const origin = startedAt ?? (fallback.current ??= Date.now());
  const [elapsedMs, setElapsedMs] = useState(() =>
    Math.max(0, Date.now() - origin),
  );

  useEffect(() => {
    const start = startedAt ?? (fallback.current ??= Date.now());
    if (paused) {
      if (pauseStarted.current == null) pauseStarted.current = Date.now();
      return;
    }
    if (pauseStarted.current != null) {
      pausedMs.current += Date.now() - pauseStarted.current;
      pauseStarted.current = null;
    }
    const tick = () =>
      setElapsedMs(Math.max(0, Date.now() - start - pausedMs.current));
    tick();
    const id = window.setInterval(tick, 1000);
    return () => window.clearInterval(id);
  }, [startedAt, paused]);

  return elapsedMs;
}

function formatWorkingDuration(
  elapsedMs: number | null,
  modelName?: string,
  done = false,
): string {
  const who = modelName?.trim();
  const elapsed = formatElapsed(elapsedMs);
  const verb = done ? (who ? "worked" : "Worked") : who ? "working" : "Working";
  if (elapsed == null) {
    if (done) return who ? `${who} ${verb}` : verb;
    return who ? `${who} ${verb}…` : `${verb}…`;
  }
  return who ? `${who} ${verb} for ${elapsed}` : `${verb} for ${elapsed}`;
}

function formatElapsed(elapsedMs: number | null): string | null {
  if (elapsedMs == null) return null;
  const totalSec = Math.max(1, Math.round(elapsedMs / 1000));
  if (totalSec < 60) return `${totalSec}s`;
  const minutes = Math.floor(totalSec / 60);
  const seconds = totalSec % 60;
  return seconds ? `${minutes}m ${seconds}s` : `${minutes}m`;
}

function ToolCall({
  block,
  cwd,
  onApproval,
  onOpenFile,
  onOpenDiff,
  embedded,
}: {
  block: Block;
  cwd?: string;
  onApproval?: (requestId: number, decision: ApprovalDecision) => void;
  onOpenFile?: (path: string) => void;
  onOpenDiff?: (path: string) => void;
  embedded?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const preview = block.tool?.preview;
  const label = toolCallLabel(block, cwd);
  const detail = block.tool?.detail?.trim();
  const expanded = detail && detail !== label ? detail : label;
  const state = toolCallState(block);
  const stateLabel =
    state === "accepted"
      ? "Accepted"
      : state === "rejected"
        ? "Rejected"
        : "Pending";
  const editTool = isEditTool(
    block.tool?.kind,
    block.text || block.tool?.title,
    preview,
  );
  const compact =
    isReadTool(block.tool?.kind, label, preview) ||
    isSearchTool(block.tool?.kind, label, preview);
  const expandable = !compact && !!detail && detail !== label;

  const frame = embedded ? "py-0.5" : "px-4 py-1";

  const appCall = monoCodeToolCall(block);
  if (appCall) {
    return (
      <div className={frame}>
        <MonoCodeCallRow block={block} call={appCall} onApproval={onApproval} />
      </div>
    );
  }

  if (editTool) {
    return (
      <div className={frame}>
        {needsApproval(block) ? (
          <FilePreview
            preview={preview ?? stubFilePreview(block.tool?.kind, label)}
            status={state}
            cwd={cwd}
            onOpenFile={onOpenDiff ?? onOpenFile}
          />
        ) : (
          <div className="flex min-w-0 items-center gap-2 py-1">
            <ToolCallIcon state={state} />
            <ToolCallSummary
              label={label}
              preview={preview}
              cwd={cwd}
              failed={state === "rejected"}
              status={state}
              onOpenFile={onOpenFile}
              onOpenDiff={onOpenDiff}
            />
          </div>
        )}
        <ApprovalControls block={block} onApproval={onApproval} />
      </div>
    );
  }

  if (isIncompleteTool(block, label, state)) return null;

  return (
    <div className={frame}>
      {expandable ? (
        <button
          type="button"
          aria-expanded={open}
          aria-label={`${stateLabel} tool call: ${label}`}
          onClick={() => setOpen((value) => !value)}
          className="flex w-full min-w-0 items-center gap-2 rounded-lg py-1.5 text-left"
        >
          <ToolCallIcon state={state} />
          <ToolCallSummary
            label={label}
            preview={preview}
            cwd={cwd}
            failed={state === "rejected"}
            onOpenFile={onOpenFile}
          />
          <ChevronRight
            className={`size-3.5 shrink-0 text-content/35 transition-transform ${open ? "rotate-90" : ""}`}
            strokeWidth={1.75}
          />
        </button>
      ) : (
        <div
          aria-label={`${stateLabel} tool call: ${label}`}
          className="flex w-full min-w-0 items-center gap-2"
        >
          <ToolCallIcon state={state} />
          <ToolCallSummary
            label={label}
            preview={preview}
            cwd={cwd}
            failed={state === "rejected"}
            onOpenFile={onOpenFile}
          />
        </div>
      )}
      {open && expandable ? (
        <pre className="mt-1.5 min-w-0 whitespace-pre-wrap break-words px-2.5 font-mono text-[12px] leading-5 text-content/55">
          {expanded}
        </pre>
      ) : null}
      <ApprovalControls block={block} onApproval={onApproval} />
    </div>
  );
}

function ToolCallSummary({
  label,
  preview,
  cwd,
  onOpenFile,
  onOpenDiff,
  interactive = true,
  chip = false,
  failed = false,
  status = "accepted",
}: {
  label: string;
  preview?: ToolPreview;
  cwd?: string;
  onOpenFile?: (path: string) => void;
  onOpenDiff?: (path: string) => void;
  interactive?: boolean;
  /** Sets the file off in a chip, for rows that lean on a rail for structure. */
  chip?: boolean;
  failed?: boolean;
  status?: ToolCallState;
}) {
  const { action, target, fileName, filePath, isFile, previewMatchesFile } =
    resolveToolCallDisplay(label, preview, cwd);
  if (!action || !target) {
    return (
      <span
        className={`min-w-0 flex-1 truncate font-mono text-[13px] ${
          failed ? "text-red-400" : chip ? "text-content/65" : "text-content/80"
        }`}
        title={label}
      >
        {label}
      </span>
    );
  }
  const openFile =
    action === "Edit" || action === "Write"
      ? (onOpenDiff ?? onOpenFile)
      : onOpenFile;
  const canOpen = interactive && !!openFile && !!filePath;
  const canPreview =
    interactive &&
    preview?.kind === "write" &&
    previewMatchesFile &&
    (preview.contentOnly ||
      preview.lines?.some((line) => line.kind !== "context"));
  const actionTone = failed ? "text-red-400" : "text-content/50";
  const targetTone = failed
    ? "text-red-400"
    : chip
      ? "text-content/70"
      : "text-content/85";

  return (
    <span className="flex min-w-0 flex-1 items-center gap-1.5 font-mono text-[13px]">
      <span className={`shrink-0 font-sans text-sm ${actionTone}`}>
        {action}
      </span>
      {isFile ? (
        canPreview ? (
          <ToolDiffPreview
            preview={preview}
            label={target}
            status={status}
            cwd={cwd}
            onOpen={openFile && filePath ? () => openFile(filePath) : undefined}
            onOpenFile={onOpenFile}
            className={`-my-0.5 flex min-w-0 cursor-pointer items-center gap-1 rounded px-1 py-0.5 text-left hover:text-sky-300 ${
              chip
                ? `max-w-full bg-content/6 hover:bg-content/10 ${targetTone}`
                : `flex-1 hover:bg-content/6 ${targetTone}`
            }`}
          >
            <FileTypeIcon name={fileName} isDir={false} />
            <span className="min-w-0 truncate">{target}</span>
          </ToolDiffPreview>
        ) : canOpen ? (
          <button
            type="button"
            className={`-my-0.5 flex min-w-0 cursor-pointer items-center gap-1 rounded px-1 py-0.5 text-left hover:text-sky-300 ${
              chip
                ? `max-w-full bg-content/6 hover:bg-content/10 ${targetTone}`
                : `flex-1 hover:underline ${targetTone}`
            }`}
            title={target}
            onClick={(event) => {
              event.stopPropagation();
              openFile?.(filePath);
            }}
          >
            <FileTypeIcon name={fileName} isDir={action === "List"} />
            <span className="min-w-0 truncate">{target}</span>
          </button>
        ) : (
          <span
            className={`flex min-w-0 items-center gap-1 rounded px-1 ${
              chip
                ? `max-w-full bg-content/6 ${targetTone}`
                : `flex-1 ${targetTone}`
            }`}
            title={target}
          >
            <FileTypeIcon name={fileName} isDir={action === "List"} />
            <span className="min-w-0 truncate">{target}</span>
          </span>
        )
      ) : (
        <span
          className={`flex min-w-0 flex-1 items-center gap-1.5 pl-1 ${targetTone}`}
          title={target}
        >
          <span className="min-w-0 truncate">{target}</span>
        </span>
      )}
    </span>
  );
}

function ToolCallIcon({ state }: { state: ToolCallState }) {
  if (state === "rejected") {
    return <X className="size-3.5 shrink-0 text-red-400" strokeWidth={2} />;
  }
  if (state === "pending") {
    return (
      <CircleDashed
        className="size-3.5 shrink-0 text-content/40"
        strokeWidth={1.75}
      />
    );
  }
  return null;
}

function ApprovalControls({
  block,
  onApproval,
}: {
  block: Block;
  onApproval?: (requestId: number, decision: ApprovalDecision) => void;
}) {
  const approval = block.approval;
  if (!approval || approval.decided || !onApproval) return null;
  return (
    <div className="mt-1.5 flex gap-2">
      <button
        type="button"
        className="rounded-md bg-content px-2.5 py-0.5 text-[11px] hover:bg-content/80     text-background-base"
        onClick={() => onApproval(approval.requestId, "allow")}
      >
        Allow
      </button>
      <button
        type="button"
        className="rounded-md bg-content/10 px-2.5 py-0.5 text-[11px] text-content/70 hover:bg-content/20"
        onClick={() => onApproval(approval.requestId, "deny")}
      >
        Deny
      </button>
    </div>
  );
}

function HandoffDivider({ block }: { block: Block }) {
  const meta = block.handoff;
  if (!meta) return null;

  const preparing = meta.status === "preparing";
  const label = preparing ? "Preparing a handoff" : HARNESS_TITLE[meta.to];

  return (
    <div className="px-4 py-5">
      <div className="flex items-center gap-3">
        <div className="h-px min-w-4 flex-1 bg-content/12" />
        <div
          role="separator"
          aria-label={
            preparing
              ? `Preparing a handoff to ${HARNESS_TITLE[meta.to]}`
              : `Continued with ${label}`
          }
          className="flex max-w-[min(100%,20rem)] items-center gap-1.5 px-1.5 font-sans text-[12px] text-content/55"
        >
          {preparing ? (
            <>
              <TerminalSpinner className="inline-block w-3.5 shrink-0 select-none text-center text-[11px] leading-none text-content/45" />
              <Shimmer duration={1.4}>{label}</Shimmer>
            </>
          ) : (
            <>
              <HarnessIcon harness={meta.to} className="size-3.5 shrink-0" />
            </>
          )}
        </div>
        <div className="h-px min-w-4 flex-1 bg-content/12" />
      </div>
    </div>
  );
}

/** The label and severity chrome an interjection wears, wherever it sits. */
function interjectionChrome(meta: InterjectionMeta): {
  label: string;
  severityText?: string;
  severityClass: string;
} {
  const label =
    meta.customType === "advisor"
      ? "Advisor"
      : meta.customType === "custom"
        ? "Notice"
        : meta.customType;
  const severityText =
    meta.severity === "blocker"
      ? "Blocker"
      : meta.severity === "concern"
        ? "Concern"
        : meta.severity === "nit"
          ? "Nit"
          : undefined;
  const severityClass =
    meta.severity === "blocker"
      ? "text-red-400"
      : meta.severity === "concern"
        ? "text-amber-400"
        : "text-content/55";
  return { label, severityText, severityClass };
}

/** The advisory body under an interjection, wherever the note is surfaced. */
const INTERJECTION_BODY =
  "min-w-0 whitespace-pre-wrap break-words font-sans text-[12.5px] leading-5 text-content/70";

/** A mid-turn interjection, e.g. OMP advisor notes: a labeled boundary with
 * a collapsible advisory body below it. */
function InterjectionDivider({ block }: { block: Block }) {
  const [expanded, setExpanded] = useState(false);
  const [overflows, setOverflows] = useState(false);
  const textRef = useRef<HTMLPreElement>(null);

  useLayoutEffect(() => {
    const el = textRef.current;
    if (!el || !block.text) {
      setOverflows(false);
      return;
    }
    const measure = () => {
      if (!expanded) {
        setOverflows(el.scrollHeight > el.clientHeight + 1);
      }
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [block.text, expanded]);

  const meta = block.interjection;
  if (!meta) return null;
  const { label, severityText, severityClass } = interjectionChrome(meta);
  return (
    <div className="px-4 py-4">
      <div className="flex items-center gap-3">
        <div className="h-px min-w-4 flex-1 bg-content/12" />
        <div
          role="separator"
          aria-label={`Interjection: ${label}`}
          className="flex items-center gap-2 px-1.5 font-sans text-[12px] text-content/55"
        >
          <span>{label}</span>
          {severityText ? (
            <span className={`text-[11px] ${severityClass}`}>
              {severityText}
            </span>
          ) : null}
        </div>
        <div className="h-px min-w-4 flex-1 bg-content/12" />
      </div>
      {block.text ? (
        <div className="mt-2 px-2">
          <pre
            ref={textRef}
            className={`${INTERJECTION_BODY} ${expanded ? "" : "line-clamp-2"}`}
          >
            {block.text}
          </pre>
          {overflows ? (
            <button
              type="button"
              aria-expanded={expanded}
              onClick={() => setExpanded((value) => !value)}
              className="mt-1 py-1 font-sans text-xs text-content/55 hover:text-content"
            >
              {expanded ? "Show less" : "Show more"}
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function lastUserBlockId(blocks: Block[], managed = false): string | undefined {
  return turnUserBlock(blocks, managed)?.id;
}

function turnUserBlock(blocks: Block[], managed = false): Block | undefined {
  for (let i = blocks.length - 1; i >= 0; i--) {
    const block = blocks[i];
    if (block.role === "user" && (managed || !block.internal)) return block;
  }
  return undefined;
}

function monoTurnUserBlock(
  blocks: Block[],
  deliveries?: ReadonlyMap<string, MessageDelivery>,
): Block | undefined {
  for (let index = blocks.length - 1; index >= 0; index--) {
    const block = blocks[index];
    if (
      block.role === "user" &&
      block.startedAt != null &&
      !deliveries?.has(block.id)
    )
      return block;
  }
  return blocks.find((block) => block.role === "user");
}

function sumDurations(durations: (number | undefined)[]): number | undefined {
  const known = durations.filter((ms): ms is number => ms != null);
  return known.length ? known.reduce((total, ms) => total + ms, 0) : undefined;
}

function userTurnCount(blocks: Block[], managed = false): number {
  return blocks.filter(
    (block) => block.role === "user" && (managed || !block.internal),
  ).length;
}

const PROMPT_RISE_MS = 560;
// Keep in sync with the prompt-turn-reveal animation in index.css.
const PROMPT_REVEAL_MS = 320;
const PROMPT_FADE_MS = 480;
// Where the prompt starts, as a fraction of the viewport height from the top.
const PROMPT_RISE_FROM = 0.3;

/** Fades the prompt in while sliding it from the upper viewport to its row. */
function riseIntoAnchor(scroller: HTMLElement | null, blockId: string) {
  const row = scroller?.querySelector<HTMLElement>(
    `[data-prompt-anchor="${CSS.escape(blockId)}"]`,
  );
  if (!scroller || !row || typeof row.animate !== "function") return;
  if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return;
  const view = scroller.getBoundingClientRect();
  const dy =
    view.top + view.height * PROMPT_RISE_FROM - row.getBoundingClientRect().top;
  if (dy <= 1) return;
  const animation = row.animate(
    [{ transform: `translateY(${dy}px)` }, { transform: "translateY(0)" }],
    { duration: PROMPT_RISE_MS, easing: "cubic-bezier(0.22, 1, 0.36, 1)" },
  );
  // The fade gets its own gentler curve; on the rise's sharp ease-out it
  // would be over before the eye catches it.
  const fade = row.animate([{ opacity: 0 }, { opacity: 1 }], {
    duration: PROMPT_FADE_MS,
    easing: "ease-out",
  });
  // The rest of the turn waits until the prompt lands, then fades in.
  const turn = row.closest<HTMLElement>(".transcript-turn");
  let revealTimer: ReturnType<typeof setTimeout> | undefined;
  turn?.setAttribute("data-prompt-rise", "rising");
  animation.onfinish = () => {
    turn?.setAttribute("data-prompt-rise", "revealing");
    revealTimer = setTimeout(
      () => turn?.removeAttribute("data-prompt-rise"),
      PROMPT_REVEAL_MS,
    );
  };
  return () => {
    animation.cancel();
    fade.cancel();
    clearTimeout(revealTimer);
    turn?.removeAttribute("data-prompt-rise");
  };
}

function followsAfterScroll(
  el: HTMLElement,
  previousTop: number,
  following: boolean,
): boolean {
  const movement = el.scrollTop - previousTop;
  if (movement === 0 || scrollClampedToBottom(el, previousTop))
    return following;
  // A small downward reversal while reading inside the bottom margin must
  // not restart following. Resume only when the reader reaches the end.
  return movement > 0 && el.scrollHeight - el.scrollTop - el.clientHeight <= 1;
}

function isNearBottom(el: HTMLElement, threshold = NEAR_BOTTOM_PX): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight <= threshold;
}

function scrollClampedToBottom(el: HTMLElement, previousTop: number): boolean {
  const bottom = Math.max(0, el.scrollHeight - el.clientHeight);
  return previousTop > bottom && Math.abs(el.scrollTop - bottom) < 1;
}

function pinToBottom(el: HTMLElement | null) {
  if (!el) return;
  el.scrollTop = el.scrollHeight;
}

/** Keep the live turn's min-height in lockstep with the visible transcript. */
function syncTranscriptViewport(el: HTMLElement | null) {
  if (!el || el.clientHeight <= 0) return;
  const inner = el.firstElementChild as HTMLElement | null;
  const pad = inner
    ? Number.parseFloat(getComputedStyle(inner).paddingBottom) || 0
    : 0;
  const next = `${Math.max(0, el.clientHeight - pad)}px`;
  if (el.style.getPropertyValue("--transcript-viewport") === next) return;
  el.style.setProperty("--transcript-viewport", next);
}

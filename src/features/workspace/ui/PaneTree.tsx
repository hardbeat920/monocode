import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useReducer,
  useRef,
  useState,
  type CSSProperties,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { setGrabbing, suppressTextSelection } from "../../../shared/lib/drag";
import { beginLayoutTransition } from "../../../shared/lib/layoutTransition";
import {
  paneDropFromPoint,
  setExternalTitleTabDrop,
  titleTabDropFromPoint,
  useExternalPaneDrop,
  type TitleTabDropPosition,
} from "../model/paneDrop";
import type {
  ApprovalDecision,
  UserQuestionReply,
} from "../../../integrations/harness";
import type { EditorNavigationTarget } from "../../search/model/search";
import {
  layoutLeaves,
  layoutSashes,
  setSplitRatio,
  type EditorPane,
  type LayoutLeaf,
  type LayoutNode,
  type LayoutRect,
  type LayoutSash,
  type PaneEdge,
} from "../model/layout";
import {
  sameProjectPath,
  type RecentProject,
} from "../../projects/model/recents";
import type { TerminalMetaPatch } from "../../terminal/model/terminalTab";
import {
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
} from "../../sessions/model/session";
import { FilePane } from "../../files/ui/FilePane";
import { SessionPane } from "../../sessions/ui/SessionPane";
import type { TranscriptPool } from "../../sessions/ui/TranscriptPool";
import type { SessionFolderTarget } from "../../sessions/model/sessionFolders";
import type { Worktree } from "../../source-control/model/worktrees";

type Shared = {
  workspaceSwitchingSessionId?: string;
  visible: boolean;
  sessions: Session[];
  editorPanes: EditorPane[];
  dirtyFileIds: Set<string>;
  fileErrorCounts: Map<string, number>;
  focusedId: string;
  addToChatSessionId?: string;
  composerFocused: boolean;
  composerFocusToken?: number;
  recents: RecentProject[];
  hideProjectPicker?: boolean;
  onFocus: (paneId: string) => void;
  onClose: (sessionId: string) => void;
  onSelectFile: (paneId: string, fileId: string) => void;
  onCloseFile: (paneId: string, fileId: string) => void;
  onCloseOtherFiles: (paneId: string, fileId: string) => void;
  onPinFile?: (fileId: string) => void;
  onReorderFiles: (paneId: string, ids: string[]) => void;
  onFileDirtyChange: (fileId: string, dirty: boolean) => void;
  onFileErrorCountChange: (fileId: string, count: number) => void;
  onRatio: (splitId: string, index: number, ratio: number) => void;
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
  onUsageLimitResume: (sessionId: string) => void;
  onUsageLimitResumeAtReset: (sessionId: string, enabled: boolean) => void;
  onUsageLimitDismiss: (sessionId: string) => void;
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
  editorNavigation?: EditorNavigationTarget | null;
  onOpenDiff: (
    path?: string,
    session?: { sessionId: string; cwd: string },
  ) => void;
  onOpenPlan: (sessionId: string, blockId: string) => void;
  onUpdatePlan: (sessionId: string, blockId: string, text: string) => void;
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
  onMovePane: (fromId: string, toId: string, edge: PaneEdge) => void;
  onDetachPane: (
    paneId: string,
    targetTabId: string,
    position: TitleTabDropPosition,
  ) => void;
  onNewTerminal: (sessionId: string) => void;
  onTerminalMetaChange?: (fileId: string, patch: TerminalMetaPatch) => void;
  transcriptPool?: TranscriptPool;
};

type Props = Shared & { layout: LayoutNode };

type PaneDrag = {
  fromId: string;
  overId: string | null;
  edge: PaneEdge;
};

const DRAG_THRESHOLD = 5;

function PaneTreeComponent({
  visible,
  layout,
  sessions,
  editorPanes,
  dirtyFileIds,
  fileErrorCounts,
  focusedId,
  addToChatSessionId,
  composerFocused,
  composerFocusToken,
  recents,
  hideProjectPicker,
  workspaceSwitchingSessionId,
  onFocus,
  onClose,
  onSelectFile,
  onCloseFile,
  onCloseOtherFiles,
  onPinFile,
  onReorderFiles,
  onFileDirtyChange,
  onFileErrorCountChange,
  onRatio,
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
  editorNavigation,
  onOpenDiff,
  onOpenPlan,
  onUpdatePlan,
  onBuildPlan,
  onSecondOpinion,
  onBtwSubmit,
  onBtwRetry,
  onBtwDelete,
  onBtwStop,
  onBtwModelChange,
  onHandoff,
  onMovePane,
  onDetachPane,
  onNewTerminal,
  onTerminalMetaChange,
  transcriptPool,
}: Props) {
  const treeRef = useRef<HTMLDivElement>(null);
  const layoutRef = useRef(layout);
  layoutRef.current = layout;
  const [draft, setDraft] = useState<LayoutNode | null>(null);
  const [paneDrag, setPaneDrag] = useState<PaneDrag | null>(null);
  const externalDrop = useExternalPaneDrop(visible);
  const drop = paneDrag ?? externalDrop;
  const onMovePaneRef = useRef(onMovePane);
  onMovePaneRef.current = onMovePane;
  const onDetachPaneRef = useRef(onDetachPane);
  onDetachPaneRef.current = onDetachPane;
  const onFocusRef = useRef(onFocus);
  onFocusRef.current = onFocus;

  useEffect(() => {
    setDraft(null);
  }, [layout]);

  // A sash drag re-renders this tree every frame. `SessionPane` compares props
  // shallowly, so handing it a fresh drag handler each frame would re-render
  // the whole session subtree (transcript, composer, picker) per frame.
  const dragHandlers = useRef(
    new Map<string, (event: ReactPointerEvent<HTMLElement>) => void>(),
  );
  const paneDragStartFor = (paneId: string) => {
    const cached = dragHandlers.current.get(paneId);
    if (cached) return cached;
    const handler = (event: ReactPointerEvent<HTMLElement>) =>
      startPaneDrag(paneId, event);
    dragHandlers.current.set(paneId, handler);
    return handler;
  };

  const tree = draft ?? layout;
  const leaves = layoutLeaves(tree);
  const sashes = layoutSashes(tree);

  // A pane split into an existing layout pushes in from the edge it was
  // added on, and a closed file pane slides back out the same way. Its
  // neighbours follow the moving edge; the sliding pane keeps its full size
  // and rides it. Panes present when the tree mounts, or swapped in place,
  // just appear.
  const knownLeaves = useRef<ReadonlyMap<string, LayoutLeaf> | null>(null);
  const shownEditorPanes = useRef(new Map<string, EditorPane>());
  const enteringPanes = useRef(new Map<string, PaneEnterFrom>());
  const leavingPanes = useRef(new Map<string, LeavingPane>());
  // A file pane slides in with its tabs but no files, and mounts its editor
  // once it has arrived, so loading the file never holds up the slide. The
  // file fades in then.
  const revealPanes = useRef(new Set<string>());
  const slideFrom = useRef<ReadonlyMap<string, LayoutLeaf> | null>(null);
  const slide = useRef<{ stop: () => void } | null>(null);
  const [, rerender] = useReducer((tick: number) => tick + 1, 0);
  const previousLeaves = knownLeaves.current;
  if (previousLeaves && visible && !draft) {
    const current = new Set(leaves.map((leaf) => leaf.id));
    const removed = [...previousLeaves.values()].filter(
      (leaf) => !current.has(leaf.id),
    );
    if (leaves.length > previousLeaves.size) {
      for (const leaf of leaves) {
        if (previousLeaves.has(leaf.id)) continue;
        enteringPanes.current.set(leaf.id, paneEnterFrom(leaf));
      }
      slideFrom.current = previousLeaves;
    } else if (
      leaves.length < previousLeaves.size &&
      removed.every((leaf) => shownEditorPanes.current.has(leaf.id))
    ) {
      // Only a file pane can outlive its layout for the slide; a session's
      // pooled transcript has already moved on.
      const order = [...previousLeaves.keys()];
      for (const leaf of removed) {
        leavingPanes.current.set(leaf.id, {
          leaf,
          pane: shownEditorPanes.current.get(leaf.id)!,
          to: paneEnterFrom(leaf),
          index: order.indexOf(leaf.id),
        });
      }
      slideFrom.current = previousLeaves;
    }
  }
  knownLeaves.current = new Map(leaves.map((leaf) => [leaf.id, leaf]));
  for (const leaf of leaves) leavingPanes.current.delete(leaf.id);
  for (const id of shownEditorPanes.current.keys()) {
    if (!knownLeaves.current.has(id) && !leavingPanes.current.has(id))
      shownEditorPanes.current.delete(id);
  }
  for (const pane of editorPanes) {
    if (knownLeaves.current.has(pane.id))
      shownEditorPanes.current.set(pane.id, pane);
  }
  const rendered: { leaf: LayoutLeaf; leaving?: LeavingPane }[] = leaves.map(
    (leaf) => ({ leaf }),
  );
  // Leaving panes keep their place, so React moves no live pane's DOM.
  for (const leaving of leavingPanes.current.values()) {
    rendered.splice(Math.min(leaving.index, rendered.length), 0, {
      leaf: leaving.leaf,
      leaving,
    });
  }
  // A closing pane still counts until it has slid out.
  const inSplit = rendered.length > 1;

  useLayoutEffect(() => {
    const from = slideFrom.current;
    if (!from) return;
    slideFrom.current = null;
    const entering = new Map(enteringPanes.current);
    const leaving = new Map(leavingPanes.current);
    // A slide still running jumps to its end; this one starts from the
    // committed layout.
    slide.current?.stop();
    for (const [id, edge] of entering) enteringPanes.current.set(id, edge);
    for (const [id, pane] of leaving) leavingPanes.current.set(id, pane);
    const tree = treeRef.current;
    const reduceMotion = window.matchMedia?.(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    const finish = (reveal = false) => {
      for (const id of entering.keys()) {
        enteringPanes.current.delete(id);
        if (reveal && shownEditorPanes.current.has(id))
          revealPanes.current.add(id);
      }
      for (const id of leaving.keys()) leavingPanes.current.delete(id);
      rerender();
    };
    if (!tree || reduceMotion || typeof tree.animate !== "function") {
      finish();
      return;
    }
    const box = tree.getBoundingClientRect();
    const opening = entering.size > 0;
    // A wide pane covers more ground than the terminal dock; it takes a
    // little longer so it moves at a similar pace.
    const travel = Math.max(
      0,
      ...[
        ...[...entering.keys()].map((id) => knownLeaves.current?.get(id)),
        ...[...leaving.values()].map((pane) => pane.leaf),
      ].map((leaf) =>
        !leaf
          ? 0
          : leaf.axis === "y"
            ? leaf.rect.h * box.height
            : leaf.rect.w * box.width,
      ),
    );
    const pace = Math.min(1.5, Math.max(1, Math.sqrt(travel / 360)));
    const timing: KeyframeAnimationOptions = {
      duration: Math.round((opening ? 240 : 200) * pace),
      easing: "cubic-bezier(0.32, 0.72, 0, 1)",
      fill: "forwards",
    };
    const animations: Animation[] = [];
    const leavingAnimations: Animation[] = [];
    const pinned: HTMLElement[] = [];
    const paneElement = (id: string) =>
      [...tree.children].find(
        (child): child is HTMLElement =>
          child instanceof HTMLElement && child.dataset.paneId === id,
      );
    // The sliding pane holds its full size against the edge it rides.
    const pin = (
      element: HTMLElement,
      rect: LayoutRect,
      edge: PaneEnterFrom,
    ) => {
      const content = element.querySelector<HTMLElement>(
        ":scope > [data-pane-content]",
      );
      if (!content) return;
      content.style.flex = "none";
      content.style.width = `${rect.w * box.width}px`;
      content.style.height = `${rect.h * box.height}px`;
      element.style.alignItems =
        edge === "left" ? "flex-end" : edge === "fade" ? "center" : "";
      element.style.justifyContent =
        edge === "top" ? "flex-end" : edge === "fade" ? "center" : "";
      // The dividers wait for the final layout; this edge stands in for
      // the one the pane rides.
      element.style.zIndex = "1";
      element.style.boxShadow = edgeShadow(edge);
      pinned.push(element);
    };
    // The sliding file's contents fade with it.
    const fade = (element: HTMLElement, show: boolean) =>
      paneBody(element)?.animate(
        [{ opacity: show ? 0 : 1 }, { opacity: show ? 1 : 0 }],
        timing,
      );
    for (const leaf of leaves) {
      const element = paneElement(leaf.id);
      if (!element) continue;
      const edge = entering.get(leaf.id);
      const start = edge ? collapsedRect(leaf, edge) : from.get(leaf.id)?.rect;
      if (!start || sameRect(start, leaf.rect)) continue;
      if (edge) {
        pin(element, leaf.rect, edge);
        const shown = shownEditorPanes.current.has(leaf.id)
          ? undefined
          : fade(element, true);
        if (shown) animations.push(shown);
      }
      animations.push(
        element.animate([rectStyle(start), rectStyle(leaf.rect)], timing),
      );
    }
    for (const pane of leaving.values()) {
      const element = paneElement(pane.leaf.id);
      if (!element) continue;
      pin(element, pane.leaf.rect, pane.to);
      const hidden = fade(element, false);
      if (hidden) leavingAnimations.push(hidden);
      // Left filled at the end: the pane is gone by the next render.
      leavingAnimations.push(
        element.animate(
          [
            rectStyle(pane.leaf.rect),
            rectStyle(collapsedRect(pane.leaf, pane.to)),
          ],
          timing,
        ),
      );
    }
    // A chat's split header comes and goes with the split; it grows or
    // shrinks with the slide instead of shifting the chat at once.
    const headers: HTMLElement[] = [];
    const splitChanges = opening ? from.size === 1 : leaves.length === 1;
    for (const leaf of splitChanges ? leaves : []) {
      if (entering.has(leaf.id)) continue;
      const header = paneElement(leaf.id)?.querySelector<HTMLElement>(
        "[data-split-header]",
      );
      if (!header) continue;
      const height = header.offsetHeight;
      header.style.overflow = "hidden";
      headers.push(header);
      const frames = [
        { height: "0px", opacity: 0 },
        { height: `${height}px`, opacity: 1 },
      ];
      const animation = header.animate(
        opening ? frames : frames.reverse(),
        timing,
      );
      // A closing split's header stays shut until it unmounts.
      (opening ? animations : leavingAnimations).push(animation);
    }
    tree.dataset.paneSliding = "";
    const endTransition = beginLayoutTransition();
    const all = [...animations, ...leavingAnimations];
    // Mounting the new pane can hold up the next frame; the slide starts
    // once that frame is out, not partway through.
    for (const animation of all) animation.pause();
    const start = requestAnimationFrame(() => {
      for (const animation of all) animation.play();
    });
    let stopped = false;
    const stop = () => {
      if (stopped) return;
      stopped = true;
      cancelAnimationFrame(start);
      for (const animation of animations) animation.cancel();
      delete tree.dataset.paneSliding;
      for (const element of pinned) {
        element.style.alignItems = "";
        element.style.justifyContent = "";
        element.style.zIndex = "";
        element.style.boxShadow = "";
        const content = element.querySelector<HTMLElement>(
          ":scope > [data-pane-content]",
        );
        content?.style.removeProperty("flex");
        content?.style.removeProperty("width");
        content?.style.removeProperty("height");
      }
      for (const header of headers) header.style.overflow = "";
      endTransition();
      if (slide.current?.stop === stop) slide.current = null;
      finish(true);
    };
    slide.current = { stop };
    // They share one timing; whichever ends first ends the slide.
    for (const animation of all) animation.onfinish = stop;
    if (!all.length) stop();
  });
  useEffect(() => () => slide.current?.stop(), []);
  useLayoutEffect(() => {
    if (!revealPanes.current.size) return;
    const tree = treeRef.current;
    const ids = [...revealPanes.current];
    revealPanes.current.clear();
    if (!tree) return;
    const fades: Animation[] = [];
    for (const child of tree.children) {
      if (
        !(child instanceof HTMLElement) ||
        !ids.includes(child.dataset.paneId!)
      )
        continue;
      const animation = paneBody(child)?.animate?.(
        [{ opacity: 0 }, { opacity: 1 }],
        {
          duration: 180,
          easing: "ease-out",
          fill: "backwards",
        },
      );
      if (animation) fades.push(animation);
    }
    // Held on its first frame until the editor's mount is out of the way.
    for (const animation of fades) animation.pause();
    requestAnimationFrame(() => {
      for (const animation of fades) animation.play();
    });
  });

  const startPaneDrag = useCallback(
    (fromId: string, event: ReactPointerEvent<HTMLElement>) => {
      if (event.button !== 0) return;
      const handle = event.currentTarget;
      const pointerId = event.pointerId;
      const startX = event.clientX;
      const startY = event.clientY;
      let active = false;

      let lastX = startX;
      let lastY = startY;
      handle.setPointerCapture(pointerId);
      const restoreSelection = suppressTextSelection();

      const onMove = (ev: PointerEvent) => {
        lastX = ev.clientX;
        lastY = ev.clientY;
        if (!active) {
          if (
            Math.hypot(ev.clientX - startX, ev.clientY - startY) <
            DRAG_THRESHOLD
          ) {
            return;
          }
          active = true;
          setGrabbing(true);
          onFocusRef.current(fromId);
          setPaneDrag({ fromId, overId: null, edge: "left" });
        }
        const titleTab = titleTabDropFromPoint(ev.clientX, ev.clientY);
        setExternalTitleTabDrop(titleTab ? { fromId, ...titleTab } : null);
        if (titleTab) {
          setPaneDrag({ fromId, overId: null, edge: "left" });
          return;
        }
        const over = paneDropFromPoint(ev.clientX, ev.clientY);
        if (!over || over.id === fromId) {
          setPaneDrag({
            fromId,
            overId: over?.id === fromId ? fromId : null,
            edge: over?.edge ?? "left",
          });
          return;
        }
        setPaneDrag({ fromId, overId: over.id, edge: over.edge });
      };

      const onUp = () => finish(true);
      const onKey = (ev: KeyboardEvent) => {
        if (ev.key !== "Escape") return;
        ev.preventDefault();
        finish(false);
      };

      function finish(commit: boolean) {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        window.removeEventListener("pointercancel", onUp);
        window.removeEventListener("keydown", onKey);
        restoreSelection();
        setGrabbing(false);
        setPaneDrag(null);
        setExternalTitleTabDrop(null);
        try {
          handle.releasePointerCapture(pointerId);
        } catch {
          /* already released */
        }
        if (!active || !commit) return;
        const titleTab = titleTabDropFromPoint(lastX, lastY);
        if (titleTab) {
          onDetachPaneRef.current(
            fromId,
            titleTab.targetTabId,
            titleTab.position,
          );
          return;
        }
        const over = paneDropFromPoint(lastX, lastY);
        if (over && over.id !== fromId) {
          onMovePaneRef.current(fromId, over.id, over.edge);
        }
      }

      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
      window.addEventListener("pointercancel", onUp);
      window.addEventListener("keydown", onKey);
    },
    [],
  );

  return (
    <div ref={treeRef} className="relative h-full min-h-0 min-w-0">
      {rendered.map(({ leaf, leaving }) => {
        const editorPane = leaving
          ? leaving.pane
          : editorPanes.find((pane) => pane.id === leaf.id);
        const session = leaving
          ? undefined
          : sessions.find((entry) => entry.id === leaf.id);
        const dragging = drop?.fromId === leaf.id;
        const onPaneDragStart =
          inSplit && !leaving ? paneDragStartFor(leaf.id) : undefined;
        const backgroundStyle = {
          "--chat-background-left": `${(-leaf.rect.x / leaf.rect.w) * 100}%`,
          "--chat-background-top": `${(-leaf.rect.y / leaf.rect.h) * 100}%`,
          "--chat-background-width": `${100 / leaf.rect.w}%`,
          "--chat-background-height": `${100 / leaf.rect.h}%`,
        } as CSSProperties;
        return (
          <div
            key={leaf.id}
            data-pane-id={leaf.id}
            data-pane-leaving={leaving ? "" : undefined}
            aria-hidden={leaving ? true : undefined}
            inert={leaving ? true : undefined}
            className={`absolute flex min-h-0 min-w-0 flex-col overflow-hidden ${dragging ? "opacity-40" : ""}${leaving ? " pointer-events-none" : ""}`}
            // The new pane focuses its composer or editor while it is still
            // offscreen, and focus scrolls this clip box to reveal it, which
            // fights the slide. Scroll events land before paint, so undoing
            // it here never shows.
            onScroll={(event) => {
              if (!enteringPanes.current.has(leaf.id)) return;
              event.currentTarget.scrollLeft = 0;
              event.currentTarget.scrollTop = 0;
            }}
            style={{
              left: `${leaf.rect.x * 100}%`,
              top: `${leaf.rect.y * 100}%`,
              width: `${leaf.rect.w * 100}%`,
              height: `${leaf.rect.h * 100}%`,
              ...backgroundStyle,
            }}
          >
            {drop && drop.overId === leaf.id && drop.fromId !== leaf.id ? (
              <PaneDropHint edge={drop.edge} />
            ) : null}
            <div
              data-pane-content=""
              data-pane-enter={
                leaving ? undefined : enteringPanes.current.get(leaf.id)
              }
              className="flex min-h-0 min-w-0 flex-1 flex-col"
            >
              {editorPane ? (
                <FilePane
                  pane={editorPane}
                  deferBody={!leaving && enteringPanes.current.has(leaf.id)}
                  visible={visible}
                  focused={!leaving && focusedId === editorPane.id}
                  showTabs={inSplit || editorPane.files.length > 1}
                  dirtyFileIds={dirtyFileIds}
                  fileErrorCounts={fileErrorCounts}
                  sessions={sessions}
                  onFocus={onFocus}
                  onSelectFile={onSelectFile}
                  onCloseFile={onCloseFile}
                  onCloseOtherFiles={onCloseOtherFiles}
                  onPinFile={onPinFile}
                  onReorderFiles={onReorderFiles}
                  onDirtyChange={onFileDirtyChange}
                  onErrorCountChange={onFileErrorCountChange}
                  onOpenFile={onOpenFile}
                  onUpdatePlan={onUpdatePlan}
                  onBuildPlan={onBuildPlan}
                  editorNavigation={editorNavigation}
                  onPaneDragStart={onPaneDragStart}
                  onTerminalMetaChange={onTerminalMetaChange}
                />
              ) : session ? (
                <SessionPane
                  session={session}
                  workspaceSwitchingSessionId={workspaceSwitchingSessionId}
                  reviewUndoLocked={sessions.some(
                    (other) =>
                      other.id !== session.id &&
                      other.busy &&
                      sameProjectPath(
                        sessionWorkCwd(other),
                        sessionWorkCwd(session),
                      ),
                  )}
                  visible={visible}
                  focused={focusedId === session.id}
                  addToChatTarget={addToChatSessionId === session.id}
                  inSplit={inSplit}
                  composerFocused={composerFocused}
                  composerFocusToken={composerFocusToken}
                  recents={recents}
                  hideProjectPicker={hideProjectPicker}
                  onFocus={onFocus}
                  onClose={onClose}
                  onCwdChange={onCwdChange}
                  onBranchChange={onBranchChange}
                  onWorktreeChange={onWorktreeChange}
                  onWorkspaceModeChange={onWorkspaceModeChange}
                  onWorktreeBaseChange={onWorktreeBaseChange}
                  onManageWorktrees={onManageWorktrees}
                  onModelChange={onModelChange}
                  onModelSettingsChange={onModelSettingsChange}
                  onRuntimeModeChange={onRuntimeModeChange}
                  onSaveDraft={onSaveDraft}
                  onRemoveDraft={onRemoveDraft}
                  onSubmit={onSubmit}
                  onStop={onStop}
                  onCompactContext={onCompactContext}
                  onPlaceSessionInFolder={onPlaceSessionInFolder}
                  onDeleteQueuedMessage={onDeleteQueuedMessage}
                  onEditQueuedMessage={onEditQueuedMessage}
                  onQueuedMessageEditingChange={onQueuedMessageEditingChange}
                  onSteerQueuedMessage={onSteerQueuedMessage}
                  onResumeQueue={onResumeQueue}
                  onUsageLimitResume={onUsageLimitResume}
                  onUsageLimitResumeAtReset={onUsageLimitResumeAtReset}
                  onUsageLimitDismiss={onUsageLimitDismiss}
                  onInboxCardDismiss={onInboxCardDismiss}
                  onLinkedWorkItemUpdateCardDismiss={
                    onLinkedWorkItemUpdateCardDismiss
                  }
                  onNoteCardDismiss={onNoteCardDismiss}
                  onHandoffCardDismiss={onHandoffCardDismiss}
                  onOpenLinkedWorkItem={onOpenLinkedWorkItem}
                  onArchiveSession={onArchiveSession}
                  onDeleteSession={onDeleteSession}
                  onApproval={onApproval}
                  onQuestionReply={onQuestionReply}
                  onQuestionInteraction={onQuestionInteraction}
                  onOpenFile={onOpenFile}
                  onOpenDiff={onOpenDiff}
                  onOpenPlan={onOpenPlan}
                  onBuildPlan={onBuildPlan}
                  onSecondOpinion={onSecondOpinion}
                  onHandoff={onHandoff}
                  onBtwSubmit={onBtwSubmit}
                  onBtwRetry={onBtwRetry}
                  onBtwDelete={onBtwDelete}
                  onBtwStop={onBtwStop}
                  onBtwModelChange={onBtwModelChange}
                  onNewTerminal={onNewTerminal}
                  onPaneDragStart={onPaneDragStart}
                  transcriptPool={transcriptPool}
                />
              ) : null}
            </div>
          </div>
        );
      })}
      {sashes.map((sash) => (
        <Sash
          key={`${sash.splitId}:${sash.index}`}
          sash={sash}
          containerRef={treeRef}
          onPreview={(ratio) =>
            setDraft(
              setSplitRatio(layoutRef.current, sash.splitId, sash.index, ratio),
            )
          }
          onCommit={(ratio) => {
            setDraft(null);
            onRatio(sash.splitId, sash.index, ratio);
          }}
          onCancel={() => setDraft(null)}
        />
      ))}
    </div>
  );
}

/** What review undo locking reads from sessions outside this tree. */
function busyWorkCwds(sessions: readonly Session[]): string {
  return sessions
    .filter((session) => session.busy)
    .map((session) => `${session.id}\0${sessionWorkCwd(session)}`)
    .join("\n");
}

/**
 * Hidden trees never re-render. A visible one skips the `sessions` rebuild
 * that another tab's stream causes, re-rendering only when its own panes'
 * sessions or the busy set it reads change.
 */
function samePaneTreeProps(previous: Props, next: Props): boolean {
  if (!previous.visible && !next.visible) return true;
  const keys = Object.keys(next) as (keyof Props)[];
  if (keys.length !== Object.keys(previous).length) return false;
  for (const key of keys) {
    if (key !== "sessions" && previous[key] !== next[key]) return false;
  }
  if (previous.sessions === next.sessions) return true;
  for (const leaf of layoutLeaves(next.layout)) {
    // File panes read the whole list.
    if (next.editorPanes.some((pane) => pane.id === leaf.id)) return false;
    const before = previous.sessions.find((entry) => entry.id === leaf.id);
    const after = next.sessions.find((entry) => entry.id === leaf.id);
    if (before !== after) return false;
  }
  return busyWorkCwds(previous.sessions) === busyWorkCwds(next.sessions);
}

export const PaneTree = memo(PaneTreeComponent, samePaneTreeProps);

type PaneEnterFrom = "left" | "right" | "top" | "bottom" | "fade";

type LeavingPane = {
  leaf: LayoutLeaf;
  pane: EditorPane;
  to: PaneEnterFrom;
  /** Its place among the leaves before it closed. */
  index: number;
};

/** What fades as a pane comes and goes: a file's body below its tabs. */
function paneBody(element: HTMLElement): HTMLElement | null {
  return (
    element.querySelector<HTMLElement>(
      ":scope > [data-pane-content] [data-pane-body]",
    ) ?? element.querySelector<HTMLElement>(":scope > [data-pane-content]")
  );
}

/** Where a pane sliding in starts, or one sliding out ends. */
function collapsedRect(
  { rect, axis }: LayoutLeaf,
  edge: PaneEnterFrom,
): LayoutRect {
  if (edge === "right") return { ...rect, x: rect.x + rect.w, w: 0 };
  if (edge === "left") return { ...rect, w: 0 };
  if (edge === "bottom") return { ...rect, y: rect.y + rect.h, h: 0 };
  if (edge === "top") return { ...rect, h: 0 };
  return axis === "y"
    ? { ...rect, y: rect.y + rect.h / 2, h: 0 }
    : { ...rect, x: rect.x + rect.w / 2, w: 0 };
}

function edgeShadow(edge: PaneEnterFrom): string {
  const line = "var(--color-stroke)";
  if (edge === "right") return `-1px 0 0 0 ${line}`;
  if (edge === "left") return `1px 0 0 0 ${line}`;
  if (edge === "bottom") return `0 -1px 0 0 ${line}`;
  if (edge === "top") return `0 1px 0 0 ${line}`;
  return `-1px 0 0 0 ${line}, 1px 0 0 0 ${line}`;
}

function rectStyle(rect: LayoutRect): Keyframe {
  return {
    left: `${rect.x * 100}%`,
    top: `${rect.y * 100}%`,
    width: `${rect.w * 100}%`,
    height: `${rect.h * 100}%`,
  };
}

function sameRect(a: LayoutRect, b: LayoutRect): boolean {
  const close = (x: number, y: number) => Math.abs(x - y) < 1e-6;
  return (
    close(a.x, b.x) && close(a.y, b.y) && close(a.w, b.w) && close(a.h, b.h)
  );
}

function paneEnterFrom({ rect, axis }: LayoutLeaf): PaneEnterFrom {
  const edge = 0.001;
  if (axis === "x") {
    if (rect.x + rect.w >= 1 - edge) return "right";
    if (rect.x <= edge) return "left";
  } else {
    if (rect.y + rect.h >= 1 - edge) return "bottom";
    if (rect.y <= edge) return "top";
  }
  return "fade";
}

function PaneDropHint({ edge }: { edge: PaneEdge }) {
  const wash =
    edge === "left"
      ? "absolute inset-y-0 left-0 w-1/2 bg-accent/15"
      : edge === "right"
        ? "absolute inset-y-0 right-0 w-1/2 bg-accent/15"
        : edge === "top"
          ? "absolute inset-x-0 top-0 h-1/2 bg-accent/15"
          : "absolute inset-x-0 bottom-0 h-1/2 bg-accent/15";
  const line =
    edge === "left"
      ? "absolute inset-y-0 left-0 w-0.5 bg-accent"
      : edge === "right"
        ? "absolute inset-y-0 right-0 w-0.5 bg-accent"
        : edge === "top"
          ? "absolute inset-x-0 top-0 h-0.5 bg-accent"
          : "absolute inset-x-0 bottom-0 h-0.5 bg-accent";
  return (
    <div className="pointer-events-none absolute inset-0 z-20">
      <div className={wash} />
      <div className={line} />
    </div>
  );
}

function Sash({
  sash,
  containerRef,
  onPreview,
  onCommit,
  onCancel,
}: {
  sash: LayoutSash;
  containerRef: { current: HTMLDivElement | null };
  onPreview: (ratio: number) => void;
  onCommit: (ratio: number) => void;
  onCancel: () => void;
}) {
  const row = sash.dir === "right";
  const boundary = sash.sizes
    .slice(0, sash.index + 1)
    .reduce((sum, size) => sum + size, 0);
  const group = sash.group;

  return (
    <div
      role="separator"
      data-pane-sash=""
      aria-orientation={row ? "vertical" : "horizontal"}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(boundary * 100)}
      className={
        row ? "absolute z-10 w-px bg-stroke" : "absolute z-10 h-px bg-stroke"
      }
      style={
        row
          ? {
              left: `${(group.x + boundary * group.w) * 100}%`,
              top: `${group.y * 100}%`,
              height: `${group.h * 100}%`,
            }
          : {
              left: `${group.x * 100}%`,
              top: `${(group.y + boundary * group.h) * 100}%`,
              width: `${group.w * 100}%`,
            }
      }
    >
      <div
        className={
          row
            ? "absolute inset-y-0 -left-1.5 -right-1.5 cursor-col-resize touch-none"
            : "absolute inset-x-0 -top-1.5 -bottom-1.5 cursor-row-resize touch-none"
        }
        onPointerDown={(e) => {
          e.preventDefault();
          e.stopPropagation();
          const handle = e.currentTarget;
          const parent = containerRef.current;
          if (!parent) return;
          handle.setPointerCapture(e.pointerId);
          const rect = parent.getBoundingClientRect();
          const restoreSelection = suppressTextSelection();
          const previousCursor = document.body.style.cursor;
          document.body.style.cursor = row ? "col-resize" : "row-resize";
          const origin = row
            ? rect.left + group.x * rect.width
            : rect.top + group.y * rect.height;
          const span = row ? group.w * rect.width : group.h * rect.height;
          let nextBoundary = boundary;
          let moved = false;
          let frame: number | null = null;

          const move = (ev: PointerEvent) => {
            const pos = row ? ev.clientX : ev.clientY;
            if (span <= 0) return;
            moved = true;
            nextBoundary = (pos - origin) / span;
            if (frame != null) return;
            frame = requestAnimationFrame(() => {
              frame = null;
              onPreview(nextBoundary);
            });
          };
          const finish = (commit: boolean) => {
            if (frame != null) {
              cancelAnimationFrame(frame);
              frame = null;
            }
            if (handle.hasPointerCapture(e.pointerId)) {
              handle.releasePointerCapture(e.pointerId);
            }
            handle.removeEventListener("pointermove", move);
            handle.removeEventListener("pointerup", up);
            handle.removeEventListener("pointercancel", cancel);
            window.removeEventListener("keydown", keydown);
            restoreSelection();
            document.body.style.cursor = previousCursor;
            if (!moved) return;
            if (commit) onCommit(nextBoundary);
            else onCancel();
          };
          const up = () => finish(true);
          const cancel = () => finish(false);
          const keydown = (event: KeyboardEvent) => {
            if (event.key !== "Escape") return;
            event.preventDefault();
            finish(false);
          };
          handle.addEventListener("pointermove", move);
          handle.addEventListener("pointerup", up);
          handle.addEventListener("pointercancel", cancel);
          window.addEventListener("keydown", keydown);
        }}
      />
    </div>
  );
}

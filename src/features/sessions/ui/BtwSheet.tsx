import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, Plus, RefreshCw, X } from "../../../shared/ui/icons";

import { Composer } from "./Composer";
import { AgentTranscript } from "./AgentTranscript";
import { GlassBackdrop } from "../../../app/shell/GlassBackdrop";
import { BtwQuestionBurst, type BurstRect } from "./BtwQuestionBurst";
import { preferredModelSettings, resolveModel } from "../model/models";
import {
  btwOpenTargetTurnId,
  btwThreadBlocks,
  btwSurfaceHarness,
  sessionBtwThreads,
} from "../model/btw";
import { groupTurns } from "../model/transcriptActivity";
import {
  DEFAULT_RUNTIME_MODE,
  HARNESS_TITLE,
  type Block,
  type BtwMessage,
  type BtwThread,
  type HarnessId,
} from "../model/session";

type Options = {
  /** False when this session cannot take side questions right now. */
  available: boolean;
  blocks: Block[];
  /** The session's current harness; each tab resolves its own from its turn. */
  harness: HarnessId;
  managed?: boolean;
  model?: string;
  modelSettings?: Record<string, string>;
  onSubmit: (
    turn: Block[],
    threadId: string,
    messageId: string,
    text: string,
    model?: string,
    modelSettings?: Record<string, string>,
  ) => boolean | void;
  onRetry: (turn: Block[], threadId: string) => void;
  onDelete?: (turn: Block[], threadId: string) => void;
  onStop?: (turn: Block[], threadId: string) => void;
  onModelChange?: (
    turn: Block[],
    threadId: string,
    model: string,
    modelSettings: Record<string, string>,
  ) => void;
};

export type BtwTab = {
  id: string;
  turn: Block[];
  thread?: BtwThread;
  question?: string;
  status?: BtwThread["status"];
};

export type BtwConversation = ReturnType<typeof useBtwConversation>;

function tabHarnessFor(
  blocks: Block[],
  tab: BtwTab,
  sessionHarness: HarnessId,
): HarnessId | undefined {
  const user = tab.turn.find((block) => block.role === "user");
  return (
    tab.thread?.harness ??
    btwSurfaceHarness(blocks, tab.turn, sessionHarness, user?.btwThreads)
  );
}

const NO_MESSAGES: BtwMessage[] = [];
const NO_BLOCKS: Block[] = [];

function reducedMotion() {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
}

/**
 * State for `/btw` side conversations. The session's own composer asks the
 * questions while this is open; the sheet shows one tab per side thread.
 */
export function useBtwConversation({
  available,
  blocks,
  harness,
  managed = false,
  model = "",
  modelSettings = {},
  onSubmit,
  onRetry,
  onDelete,
  onStop,
  onModelChange,
}: Options) {
  const [requestedOpen, setRequestedOpen] = useState(false);
  const open = requestedOpen && available;
  // Stay mounted while the close animation plays.
  const [rendered, setRendered] = useState(false);
  if (open && !rendered) setRendered(true);
  const [drafts, setDrafts] = useState<{ id: string; turnId: string }[]>([]);
  const draftTextsRef = useRef<Record<string, string>>({});
  const [activeId, setActiveId] = useState<string | null>(null);
  const [draftModel, setDraftModel] = useState<string | null>(null);
  const [draftModelSettings, setDraftModelSettings] = useState<Record<
    string,
    string
  > | null>(null);
  const [optimistic, setOptimistic] = useState<{
    threadId: string;
    message: BtwMessage;
  } | null>(null);
  // Unsent text the side composer starts from; a new key remounts it.
  const [seed, setSeed] = useState({ key: 0, text: "" });

  const turns = useMemo(() => groupTurns(blocks, managed), [blocks, managed]);
  const entries = useMemo(
    () => sessionBtwThreads(blocks, managed),
    [blocks, managed],
  );
  const tabs = useMemo(() => {
    const next: BtwTab[] = entries.map(({ thread, turn }) => ({
      id: thread.id,
      turn,
      thread,
    }));
    for (const draft of drafts) {
      if (next.some((tab) => tab.id === draft.id)) continue;
      const turn = turns.find((entry) => entry[0]?.id === draft.turnId);
      if (turn) next.push({ id: draft.id, turn });
    }
    return next.map((tab) => {
      const pending = optimistic?.threadId === tab.id ? optimistic : null;
      return {
        ...tab,
        question:
          tab.thread?.messages.find((message) => message.role === "user")
            ?.text ?? pending?.message.text,
        status: tab.thread?.status ?? (pending ? "running" : undefined),
      };
    });
  }, [drafts, entries, optimistic, turns]);
  const active =
    tabs.find((tab) => tab.id === activeId) ?? tabs[tabs.length - 1];
  const activeTabId = active?.id ?? null;
  const persisted = active?.thread;
  const tabHarness = active
    ? tabHarnessFor(blocks, active, harness)
    : undefined;
  const composerHarness = tabHarness ?? harness;
  const baseModelFor = (tab: BtwTab) =>
    tab.turn.find((block) => block.role === "user")?.turnModel?.id ?? model;

  const optimisticForTab =
    optimistic?.threadId === activeTabId ? optimistic.message : undefined;
  const persistedMessages = persisted?.messages ?? NO_MESSAGES;
  // Stable while nothing changes, so the transcript is not re-fed each render.
  const messages = useMemo(
    () =>
      optimisticForTab &&
      !persistedMessages.some((message) => message.id === optimisticForTab.id)
        ? [...persistedMessages, optimisticForTab]
        : persistedMessages,
    [optimisticForTab, persistedMessages],
  );
  const running = persisted?.status === "running" || !!optimisticForTab;
  const selectedModel =
    draftModel ?? persisted?.model ?? (active ? baseModelFor(active) : model);
  const selectedModelSettings = useMemo(() => {
    if (draftModelSettings) return draftModelSettings;
    if (persisted?.modelSettings) return persisted.modelSettings;
    return preferredModelSettings(
      resolveModel(composerHarness, selectedModel),
      modelSettings,
    );
  }, [
    composerHarness,
    draftModelSettings,
    modelSettings,
    persisted?.modelSettings,
    selectedModel,
  ]);

  useEffect(() => {
    if (!available) setRequestedOpen(false);
  }, [available]);

  useEffect(() => {
    if (
      optimistic &&
      entries.some(
        ({ thread }) =>
          thread.id === optimistic.threadId &&
          thread.messages.some(
            (message) => message.id === optimistic.message.id,
          ),
      )
    ) {
      setOptimistic(null);
    }
  }, [optimistic, entries]);

  useEffect(() => {
    const persistedIds = new Set(entries.map(({ thread }) => thread.id));
    setDrafts((current) => {
      const pending = current.filter((draft) => !persistedIds.has(draft.id));
      return pending.length === current.length ? current : pending;
    });
  }, [entries]);

  const finishClose = () => {
    // Availability can disappear without an explicit close (for example when
    // the session changes harness). Do not leave a latent open request that
    // reopens the sheet if availability later returns.
    setRequestedOpen(false);
    setRendered(false);
    setDrafts([]);
    draftTextsRef.current = {};
    setOptimistic(null);
    setDraftModel(null);
    setDraftModelSettings(null);
  };

  const send = (
    tab: BtwTab,
    text: string,
    nextModel: string,
    nextSettings: Record<string, string>,
  ): boolean => {
    const messageId = crypto.randomUUID();
    const accepted = onSubmit(
      tab.turn,
      tab.id,
      messageId,
      text,
      nextModel || undefined,
      nextSettings,
    );
    if (accepted === false) return false;
    setOptimistic({
      threadId: tab.id,
      message: { id: messageId, role: "user", text, createdAt: Date.now() },
    });
    return true;
  };

  // A new tab reads from the latest finished turn at the moment it opens.
  const startDraft = (): BtwTab | null => {
    const turnId = btwOpenTargetTurnId(turns, blocks, harness, managed);
    const turn = turnId
      ? turns.find((entry) => entry[0]?.id === turnId)
      : undefined;
    if (!turnId || !turn) return null;
    const id = crypto.randomUUID();
    setDrafts((current) => [...current, { id, turnId }]);
    setActiveId(id);
    setDraftModel(null);
    setDraftModelSettings(null);
    draftTextsRef.current[id] = "";
    setSeed((current) => ({ key: current.key + 1, text: "" }));
    return { id, turn };
  };

  const selectTab = (id: string) => {
    if (id === activeTabId) return;
    setDraftModel(null);
    setDraftModelSettings(null);
    setActiveId(id);
    setSeed((current) => ({
      key: current.key + 1,
      text: draftTextsRef.current[id] ?? "",
    }));
  };

  return {
    open,
    rendered,
    tabs,
    activeTabId,
    persisted,
    messages,
    pendingBlocks: persisted?.pendingBlocks ?? NO_BLOCKS,
    running,
    harness: composerHarness,
    canAsk: tabHarness != null,
    seed,
    model: selectedModel,
    modelSettings: selectedModelSettings,
    canStartDraft: btwOpenTargetTurnId(turns, blocks, harness, managed) != null,
    finishClose,
    selectTab,
    /**
     * Open a new tab. `text` is sent right away, or left unsent in the side
     * composer with `draft`.
     */
    openWith(text: string, options?: { draft?: boolean }): boolean {
      if (!available) return false;
      const question = text.trim();
      if (!question && !btwOpenTargetTurnId(turns, blocks, harness, managed)) {
        const existing =
          tabs.find((tab) => tab.id === activeTabId) ?? tabs[tabs.length - 1];
        if (!existing) return false;
        setActiveId(existing.id);
        setSeed((current) => ({
          key: current.key + 1,
          text: draftTextsRef.current[existing.id] ?? "",
        }));
        setRequestedOpen(true);
        return true;
      }
      const previousActiveId = activeTabId;
      const tab = startDraft();
      if (!tab) return false;
      if (options?.draft) {
        setRequestedOpen(true);
        draftTextsRef.current[tab.id] = text;
        setSeed((current) => ({ key: current.key + 1, text }));
        return true;
      }
      const tabModel = baseModelFor(tab);
      const tabHarness = tabHarnessFor(blocks, tab, harness);
      if (question && tabHarness) {
        const accepted = send(
          tab,
          question,
          tabModel,
          preferredModelSettings(
            resolveModel(tabHarness, tabModel),
            modelSettings,
          ),
        );
        if (!accepted) {
          setDrafts((current) =>
            current.filter((entry) => entry.id !== tab.id),
          );
          delete draftTextsRef.current[tab.id];
          setActiveId(previousActiveId);
          return false;
        }
      }
      setRequestedOpen(true);
      return true;
    },
    startDraft() {
      startDraft();
    },
    changeDraft(text: string) {
      if (activeTabId) draftTextsRef.current[activeTabId] = text;
    },
    close() {
      setRequestedOpen(false);
    },
    /** False keeps the text in the composer. */
    submit(text: string): boolean {
      const question = text.trim();
      if (!question || running || !active || !tabHarness) return false;
      return send(active, question, selectedModel, selectedModelSettings);
    },
    closeTab(tab: BtwTab) {
      const remaining = tabs.filter((entry) => entry.id !== tab.id);
      setDrafts((current) => current.filter((entry) => entry.id !== tab.id));
      if (tab.thread) onDelete?.(tab.turn, tab.id);
      delete draftTextsRef.current[tab.id];
      if (remaining.length === 0) {
        setRequestedOpen(false);
        return;
      }
      if (tab.id === activeTabId) {
        const index = tabs.findIndex((entry) => entry.id === tab.id);
        selectTab(remaining[Math.min(index, remaining.length - 1)].id);
      }
    },
    /** Stop the active tab's streaming answer, keeping what arrived. */
    stop() {
      if (!active) return;
      setOptimistic((current) =>
        current?.threadId === active.id ? null : current,
      );
      if (persisted?.status === "running") onStop?.(active.turn, active.id);
    },
    retry() {
      if (active && persisted) onRetry(active.turn, persisted.id);
    },
    changeModel(nextHarness: HarnessId, nextModel: string) {
      if (nextHarness !== composerHarness) return;
      const nextSettings = preferredModelSettings(
        resolveModel(nextHarness, nextModel),
        selectedModelSettings,
      );
      setDraftModel(nextModel);
      setDraftModelSettings(nextSettings);
      if (persisted && active) {
        onModelChange?.(active.turn, active.id, nextModel, nextSettings);
      }
    },
    changeModelSettings(nextSettings: Record<string, string>) {
      setDraftModelSettings(nextSettings);
      if (persisted && active) {
        onModelChange?.(active.turn, active.id, selectedModel, nextSettings);
      }
    },
  };
}

function compactQuestion(text: string | undefined): string {
  const compact = text?.replace(/\s+/g, " ").trim() || "New question";
  return compact.length > 48 ? `${compact.slice(0, 45)}…` : compact;
}

function statusClass(status: BtwThread["status"]): string {
  if (status === "running") return "bg-amber-300";
  if (status === "error") return "bg-red-300";
  return "bg-emerald-300";
}

const OPEN_MOTION = {
  duration: 360,
  easing: "cubic-bezier(0.22, 1, 0.36, 1)",
};
const CLOSE_MOTION = {
  duration: 260,
  easing: "cubic-bezier(0.4, 0, 0.2, 1)",
  fill: "forwards" as const,
};

/** An element's box relative to a host element. */
function rectIn(el: HTMLElement, host: HTMLElement): BurstRect {
  const rect = el.getBoundingClientRect();
  const frame = host.getBoundingClientRect();
  return {
    left: rect.left - frame.left,
    top: rect.top - frame.top,
    width: rect.width,
    height: rect.height,
  };
}

function box(rect: BurstRect) {
  return {
    left: `${rect.left}px`,
    top: `${rect.top}px`,
    width: `${rect.width}px`,
    height: `${rect.height}px`,
  };
}

/** The full sheet and the composer box it grows out of, as clip paths. */
function morphClips(sheet: HTMLElement, origin: HTMLElement | null) {
  if (!origin) return null;
  const box = origin.getBoundingClientRect();
  const frame = sheet.getBoundingClientRect();
  if (!box.width || !box.height || !frame.height) return null;
  const radius = parseFloat(getComputedStyle(origin).borderTopLeftRadius) || 0;
  const r = `${radius}px`;
  const top = Math.max(0, box.top - frame.top);
  const right = Math.max(0, frame.right - box.right);
  const bottom = Math.max(0, frame.bottom - box.bottom);
  const left = Math.max(0, box.left - frame.left);
  return {
    box: `inset(${top}px ${right}px ${bottom}px ${left}px round ${r} ${r} ${r} ${r})`,
    full: `inset(0px 0px 0px 0px round ${r} ${r} 0px 0px)`,
  };
}

/**
 * The side conversation over the transcript: tabs for every side thread in
 * the session, the active thread, and a composer for it. It morphs out of
 * the session's composer box and folds back into it on close.
 */
export function BtwSheet({
  btw,
  cwd,
  visible = true,
  origin,
  onSaveNote,
  onOpenFile,
  onOpenDiff,
}: {
  btw: BtwConversation;
  cwd?: string;
  visible?: boolean;
  /** The session composer's box, which the sheet grows out of. */
  origin?: () => HTMLElement | null;
  onSaveNote?: (text: string) => void | Promise<void>;
  onOpenFile?: (path: string) => void;
  onOpenDiff?: (path: string) => void;
}) {
  const sheetRef = useRef<HTMLElement | null>(null);
  const glassRef = useRef<HTMLDivElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const overlayRef = useRef<HTMLDivElement | null>(null);
  const frameRef = useRef<HTMLDivElement | null>(null);
  const motionRef = useRef<Animation[] | null>(null);
  // The session composer stays hidden under an open sheet, past the morph.
  const homeFadeRef = useRef<Animation | null>(null);
  const startRef = useRef<number[]>([]);
  const [burst, setBurst] = useState<(BurstRect & { key: number }) | null>(
    null,
  );
  const { open, rendered, tabs, activeTabId, messages, persisted, running } =
    btw;
  const harness = btw.harness;
  const finishClose = btw.finishClose;
  // The side thread renders through the main transcript, so it anchors,
  // follows, and folds its work exactly like the conversation behind it.
  const threadBlocks = useMemo(
    () =>
      btwThreadBlocks({
        messages,
        pendingBlocks: btw.pendingBlocks,
        running,
        updatedAt: persisted?.updatedAt,
        harness,
        model: btw.model,
      }),
    [
      messages,
      btw.pendingBlocks,
      running,
      persisted?.updatedAt,
      harness,
      btw.model,
    ],
  );

  // Morph between the composer box and the full sheet. The panel grows out
  // of the session composer's box and folds back into it; a frame in the
  // composer's shape carries the input from one box to the other while the
  // two composers cross-fade inside it, so the input itself reads as
  // reshaping. Opening also sets off a burst of question marks. Starting
  // from the current frame lets a quick reopen or close reverse mid-flight.
  // Like popovers, the glass layer is never inside an animating ancestor:
  // it and the content each run the same morph.
  useLayoutEffect(() => {
    const sheet = sheetRef.current;
    const overlay = overlayRef.current;
    const frame = frameRef.current;
    const layers = [glassRef.current, contentRef.current].filter(
      (layer): layer is HTMLDivElement => layer != null,
    );
    if (!rendered || !sheet || !overlay || layers.length === 0) return;
    const originBox = origin?.() ?? null;
    const clips = morphClips(sheet, originBox);
    const running = motionRef.current;
    if (!clips || reducedMotion() || typeof sheet.animate !== "function") {
      running?.forEach((motion) => motion.cancel());
      motionRef.current = null;
      if (!open) {
        homeFadeRef.current?.cancel();
        homeFadeRef.current = null;
        finishClose();
      }
      return;
    }
    const style = running ? getComputedStyle(layers[0]) : null;
    const clip =
      style?.clipPath && style.clipPath !== "none" ? style.clipPath : null;
    const opacity = style ? Number(style.opacity) : null;
    const frameNow = running && frame ? rectIn(frame, overlay) : null;
    running?.forEach((motion) => motion.cancel());
    startRef.current.forEach((id) => cancelAnimationFrame(id));
    startRef.current = [];
    delete overlay.dataset.morph;
    const timing = open ? OPEN_MOTION : CLOSE_MOTION;
    const frames = open
      ? [
          { clipPath: clip ?? clips.box, opacity: opacity ?? 0 },
          { opacity: 1, offset: 0.3 },
          { clipPath: clips.full, opacity: 1 },
        ]
      : [
          { clipPath: clip ?? clips.full, opacity: opacity ?? 1 },
          { opacity: 1, offset: 0.2 },
          { clipPath: clips.box, opacity: 0 },
        ];
    const motions = layers.map((layer) => layer.animate(frames, timing));

    const sheetComposer = contentRef.current?.querySelector<HTMLElement>(
      ".btw-sheet-composer",
    );
    const sheetBox = sheetComposer?.querySelector<HTMLElement>(
      "[data-composer-box]",
    );
    if (frame && originBox && sheetBox) {
      const home = rectIn(originBox, overlay);
      const away = rectIn(sheetBox, overlay);
      const from = frameNow ?? (open ? home : away);
      const to = open ? away : home;
      motions.push(
        frame.animate(
          [
            { ...box(from), opacity: 0 },
            { opacity: 1, offset: 0.15 },
            { opacity: 1, offset: 0.8 },
            { ...box(to), opacity: 0 },
          ],
          timing,
        ),
      );
      if (open) {
        // The side composer surfaces inside the frame once it has mostly
        // taken the new shape.
        motions.push(
          sheetComposer!.animate(
            [{ opacity: 0 }, { opacity: 0, offset: 0.45 }, { opacity: 1 }],
            timing,
          ),
        );
        if (!running) setBurst({ key: Date.now(), ...away });
      }
    }

    const home = originBox?.closest<HTMLElement>("[data-session-composer]");
    if (home) {
      homeFadeRef.current?.cancel();
      const fade = home.animate(
        open
          ? [{ opacity: 1 }, { opacity: 0, offset: 0.3 }, { opacity: 0 }]
          : [{ opacity: 0 }, { opacity: 0, offset: 0.55 }, { opacity: 1 }],
        { ...timing, fill: open ? "forwards" : "none" },
      );
      homeFadeRef.current = open ? fade : null;
      motions.push(fade);
    }

    // Opening mounts the whole sheet, and its first paint can take longer
    // than a frame. Hold everything on its first frame until that paint has
    // landed, so the morph plays from the start instead of mid-way.
    if (open) {
      motions.forEach((motion) => motion.pause());
      overlay.dataset.morph = "pending";
      startRef.current = [
        requestAnimationFrame(() => {
          startRef.current = [
            requestAnimationFrame(() => {
              startRef.current = [];
              delete overlay.dataset.morph;
              motions.forEach((motion) => motion.play());
            }),
          ];
        }),
      ];
    }

    motionRef.current = motions;
    motions[0].onfinish = () => {
      if (motionRef.current !== motions) return;
      motionRef.current = null;
      if (!open) finishClose();
    };
  }, [open, rendered]);

  // Esc collapses the sheet from anywhere in this pane, and focus follows
  // the sheet in so typing goes to the side question.
  useEffect(() => {
    const sheet = sheetRef.current;
    if (!open || !visible || !sheet) return;
    const frame = requestAnimationFrame(() => {
      if (!sheet.contains(document.activeElement)) {
        sheet.querySelector<HTMLTextAreaElement>("textarea")?.focus();
      }
    });
    const pane = sheet.closest("[data-session-drop]") ?? sheet.parentElement;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      if (event.isComposing) return;
      const target = event.target instanceof Node ? event.target : null;
      if (target !== document.body && !(target && pane?.contains(target))) {
        return;
      }
      event.preventDefault();
      btw.close();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("keydown", onKey);
    };
  }, [open, visible, activeTabId, btw.seed.key]);

  if (!rendered) return null;

  return (
    <div
      data-btw-overlay
      data-state={open ? "open" : "closed"}
      inert={!open}
      ref={overlayRef}
      className="btw-overlay absolute inset-0 z-40"
    >
      <div aria-hidden className="absolute inset-0" onMouseDown={btw.close} />
      <section
        ref={sheetRef}
        role="dialog"
        aria-label="By-the-way conversations"
        className="btw-sheet absolute inset-x-0 bottom-0 isolate mx-auto w-full max-w-4xl"
      >
        <GlassBackdrop
          ref={glassRef}
          className="btw-sheet-glass popover-backdrop"
        />
        <div
          ref={contentRef}
          className="relative z-[1] flex h-full flex-col font-sans text-sm text-content"
        >
          <div className="btw-sheet-body flex h-11 shrink-0 items-center gap-1 border-b border-content/8 pr-2 pl-2.5">
            <div
              role="tablist"
              aria-label="Side questions"
              className="scrollbar-none flex min-w-0 flex-1 items-center gap-1 overflow-x-auto"
            >
              {tabs.map((tab) => {
                const selected = tab.id === activeTabId;
                const label = compactQuestion(tab.question);
                return (
                  <div
                    key={tab.id}
                    className={`group flex h-7 max-w-[15rem] shrink-0 items-center rounded-md transition-colors ${
                      selected
                        ? "bg-content/10 text-content"
                        : "text-content/50 hover:bg-content/5 hover:text-content/80"
                    }`}
                  >
                    <button
                      type="button"
                      role="tab"
                      aria-selected={selected}
                      title={tab.question}
                      onClick={() => btw.selectTab(tab.id)}
                      className="flex h-full min-w-0 items-center gap-1.5 rounded-md pr-1 pl-2.5 text-xs focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent"
                    >
                      {tab.status ? (
                        <span
                          aria-hidden
                          className={`size-1.5 shrink-0 rounded-full ${statusClass(tab.status)}`}
                        />
                      ) : null}
                      <span className="truncate">{label}</span>
                    </button>
                    <button
                      type="button"
                      aria-label={
                        tab.thread
                          ? `Delete “${label}”`
                          : "Discard new question"
                      }
                      title={tab.thread ? "Delete" : "Discard"}
                      onClick={() => btw.closeTab(tab)}
                      className={`mr-1 grid size-5 shrink-0 place-items-center rounded text-content/40 transition-opacity hover:bg-content/10 hover:text-content focus-visible:opacity-100 focus-visible:outline-2 focus-visible:outline-accent ${
                        selected ? "" : "opacity-0 group-hover:opacity-100"
                      }`}
                    >
                      <X className="size-3" strokeWidth={1.75} />
                    </button>
                  </div>
                );
              })}
            </div>
            {btw.canStartDraft ? (
              <button
                type="button"
                aria-label="New side question"
                title="New side question"
                onClick={btw.startDraft}
                className="grid size-7 shrink-0 place-items-center rounded-md text-content/45 transition-colors hover:bg-content/8 hover:text-content focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent"
              >
                <Plus className="size-3.5" strokeWidth={1.75} />
              </button>
            ) : null}
            <button
              type="button"
              aria-label="Back to the conversation"
              title="Back to the conversation (Esc)"
              onClick={btw.close}
              className="grid size-7 shrink-0 place-items-center rounded-md text-content/45 transition-colors hover:bg-content/8 hover:text-content focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent"
            >
              <ChevronDown className="size-4" strokeWidth={1.75} />
            </button>
          </div>

          <div className="btw-sheet-body min-h-0 flex-1">
            <AgentTranscript
              key={activeTabId ?? ""}
              blocks={threadBlocks}
              busy={running}
              visible={visible && open}
              cwd={cwd}
              harness={harness}
              model={btw.model}
              modelSettings={btw.modelSettings}
              onSaveNote={onSaveNote}
              onOpenFile={onOpenFile}
              onOpenDiff={onOpenDiff}
            />
          </div>
          {persisted?.status === "error" ? (
            <div className="btw-sheet-body shrink-0 px-5 pb-3">
              <div className="btw-error" role="alert">
                <div className="min-w-0">
                  <div className="text-[10px] font-medium uppercase tracking-[0.08em] text-red-200/70">
                    Couldn’t finish
                  </div>
                  <div className="mt-1 text-[12px] leading-4.5 text-red-100/75">
                    {persisted.error ||
                      `${HARNESS_TITLE[harness]} could not answer this side question.`}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={btw.retry}
                  className="inline-flex h-6 shrink-0 items-center gap-1 rounded-md px-2 text-[11px] font-medium text-red-100/80 transition-colors hover:bg-red-200/10 hover:text-red-50 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-red-200/60"
                >
                  <RefreshCw className="size-3" strokeWidth={1.75} />
                  Retry
                </button>
              </div>
            </div>
          ) : null}

          {/* Same inset as the docked composer, so the morph starts on its box. */}
          <div className="btw-sheet-composer shrink-0 p-1.5 pt-0">
            {activeTabId ? (
              <Composer
                key={`${activeTabId}:${btw.seed.key}`}
                compact
                enabled={visible && open}
                disabled={!btw.canAsk}
                focused={visible && open && !running}
                harness={harness}
                model={btw.model}
                modelSettings={btw.modelSettings}
                runtimeMode={DEFAULT_RUNTIME_MODE}
                cwd={cwd ?? "~"}
                executionCwd={cwd ?? "~"}
                sessionId={activeTabId}
                hideProjectPicker
                hideBranchPicker
                hideTopBar
                placeholder="Ask a side question…"
                inputAriaLabel="By-the-way question"
                allowedModelHarnesses={[harness]}
                initialDraft={btw.seed.text}
                onDraftChange={btw.changeDraft}
                onFocus={() => {}}
                onCwdChange={() => {}}
                onModelChange={btw.changeModel}
                onModelSettingsChange={btw.changeModelSettings}
                onRuntimeModeChange={() => {}}
                busy={running}
                allowBusySubmit={false}
                onStop={btw.stop}
                onSubmit={(text) => btw.submit(text)}
              />
            ) : null}
          </div>
        </div>
      </section>
      <div ref={frameRef} aria-hidden className="btw-morph-frame" />
      {burst ? (
        <BtwQuestionBurst
          key={burst.key}
          rect={burst}
          onDone={() => setBurst(null)}
        />
      ) : null}
    </div>
  );
}

import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  AlertCircle,
  LoaderCircle,
  Plus,
  RefreshCw,
  X,
} from "../../../shared/ui/icons";
import { GlassBackdrop } from "../../../app/shell/GlassBackdrop";
import { LAYER } from "../../../shared/lib/layers";
import {
  BOARD_LANES,
  type BoardStatus,
  type ProjectBoardCard,
  type ProjectBoardDialogProps,
} from "./types";
import {
  defaultProjectBoardRepository,
  subscribeProjectBoard,
} from "./defaultRepository";
import { BoardCard } from "./BoardCard";
import { CardEditorModal } from "./CardEditorModal";
import { DeleteConfirmModal } from "./DeleteConfirmModal";
import { useDialogFocusTrap } from "./useDialogFocusTrap";

export function ProjectBoardDialog({
  projectCwd,
  onClose,
  onOpenSession,
  repository = defaultProjectBoardRepository,
}: ProjectBoardDialogProps) {
  const [cards, setCards] = useState<ProjectBoardCard[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [editingCard, setEditingCard] = useState<ProjectBoardCard | null>(null);
  const [isCreating, setIsCreating] = useState(false);
  const [createStatus, setCreateStatus] = useState<BoardStatus>("backlog");

  const [pendingDeleteCard, setPendingDeleteCard] = useState<ProjectBoardCard | null>(null);
  const [isDeletingCard, setIsDeletingCard] = useState(false);

  const dialogRef = useRef<HTMLDivElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const uid = useId();
  const titleId = `${uid}-board-title`;
  const descId = `${uid}-board-desc`;

  useDialogFocusTrap(dialogRef, {
    onClose,
    initialFocusRef: closeButtonRef,
  });

  const loadCards = useCallback(async () => {
    setError(null);
    try {
      const list = await repository.list(projectCwd);
      setCards(list);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load board cards");
    } finally {
      setLoading(false);
    }
  }, [projectCwd, repository]);

  useEffect(() => {
    loadCards();
  }, [loadCards]);

  // Live refresh on monocode:project-board-changed
  useEffect(() => {
    return subscribeProjectBoard(projectCwd, loadCards);
  }, [loadCards, projectCwd]);

  // Group cards into lanes
  const cardsByLane = useMemo(() => {
    const map: Record<BoardStatus, ProjectBoardCard[]> = {
      backlog: [],
      ready: [],
      "in-progress": [],
      blocked: [],
      done: [],
    };
    for (const card of cards) {
      if (map[card.status]) {
        map[card.status].push(card);
      } else {
        map.backlog.push(card);
      }
    }
    return map;
  }, [cards]);

  const handleMoveCard = async (
    card: ProjectBoardCard,
    nextStatus: BoardStatus,
  ) => {
    if (card.status === nextStatus) return;
    setError(null);
    try {
      const updated = await repository.patchCard(card.projectCwd, card.id, {
        status: nextStatus,
      });
      setCards((prev) =>
        prev.map((current) => (current.id === card.id ? updated : current)),
      );
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to move card");
    }
  };

  const handleDeleteCard = (cardId: string) => {
    const target = cards.find((c) => c.id === cardId);
    if (target) {
      setPendingDeleteCard(target);
    }
  };

  const handleConfirmDelete = async () => {
    if (!pendingDeleteCard) return;
    const cardId = pendingDeleteCard.id;
    setIsDeletingCard(true);
    try {
      await repository.deleteCard(projectCwd, cardId);
      setCards((prev) => prev.filter((c) => c.id !== cardId));
      setPendingDeleteCard(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to delete card");
    } finally {
      setIsDeletingCard(false);
    }
  };

  const handleCardSaved = (savedCard: ProjectBoardCard) => {
    setCards((prev) => {
      const index = prev.findIndex((c) => c.id === savedCard.id);
      if (index >= 0) {
        const next = [...prev];
        next[index] = savedCard;
        return next;
      }
      return [...prev, savedCard];
    });
  };

  const handleMediaDeleted = useCallback((cardId: string, mediaId: string) => {
    setCards((prev) =>
      prev.map((c) =>
        c.id === cardId
          ? {
              ...c,
              media: (c.media ?? []).filter((m) => m.id !== mediaId),
            }
          : c,
      ),
    );
  }, []);

  const handleOpenCreate = (status: BoardStatus = "backlog") => {
    setCreateStatus(status);
    setIsCreating(true);
  };

  const totalCards = cards.length;

  return (
    <div
      ref={dialogRef}
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      aria-describedby={descId}
      className="fixed inset-0 flex items-center justify-center p-3 sm:p-5"
      style={{ zIndex: LAYER.dialog }}
    >
      {/* Dimmed backdrop */}
      <div
        className="absolute inset-0 bg-black/60 backdrop-blur-sm"
        onClick={onClose}
      />

      {/* Main Workspace Dialog Container */}
      <div className="relative isolate z-10 flex h-[min(880px,calc(100dvh-24px))] w-[min(1440px,calc(100vw-24px))] flex-col overflow-hidden rounded-2xl border border-content/10 bg-background-base shadow-2xl">
        <GlassBackdrop className="bg-background-base/95" />

        {/* Dialog Header */}
        <header className="relative z-10 flex shrink-0 items-center justify-between border-b border-content/8 px-4 py-3 sm:px-6">
          <div className="min-w-0 flex-1 pr-4">
            <div className="flex items-center gap-2.5">
              <h2
                id={titleId}
                className="text-base font-semibold leading-tight text-content sm:text-lg"
              >
                Project Board
              </h2>
              <span className="rounded-full bg-content/8 px-2 py-0.5 text-xs font-medium text-content/60 tabular-nums">
                {totalCards} {totalCards === 1 ? "card" : "cards"}
              </span>
            </div>
            <p
              id={descId}
              className="truncate text-xs text-content/50"
              title={projectCwd}
            >
              {projectCwd}
            </p>
          </div>

          <div className="flex items-center gap-2">
            <button
              type="button"
              aria-label="Refresh project board"
              title="Refresh project board"
              onClick={loadCards}
              disabled={loading}
              className="grid size-8 place-items-center rounded-lg border border-content/10 bg-content/4 text-content/70 hover:bg-content/8 hover:text-content focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-40"
            >
              <RefreshCw
                className={`size-3.5 ${loading ? "animate-spin" : ""}`}
                strokeWidth={1.75}
              />
            </button>

            <button
              type="button"
              aria-label="Add new card"
              onClick={() => handleOpenCreate("backlog")}
              className="flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white shadow-sm hover:brightness-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            >
              <Plus className="size-3.5" strokeWidth={2} />
              <span className="hidden sm:inline">New card</span>
            </button>

            <button
              ref={closeButtonRef}
              type="button"
              aria-label="Close project board"
              title="Close project board"
              onClick={onClose}
              className="grid size-8 place-items-center rounded-lg text-content/50 hover:bg-content/8 hover:text-content focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            >
              <X className="size-4" strokeWidth={1.75} />
            </button>
          </div>
        </header>

        {/* Error Banner */}
        {error ? (
          <div
            role="alert"
            className="relative z-10 flex shrink-0 items-center justify-between border-b border-red-500/20 bg-red-500/10 px-4 py-2.5 text-xs text-red-400 sm:px-6"
          >
            <div className="flex items-center gap-2">
              <AlertCircle className="size-4 shrink-0" />
              <span>{error}</span>
            </div>
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={loadCards}
                className="font-medium underline hover:no-underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-red-400"
              >
                Retry
              </button>
              <button
                type="button"
                aria-label="Dismiss error"
                onClick={() => setError(null)}
                className="grid size-5 place-items-center rounded hover:bg-red-500/20"
              >
                <X className="size-3" />
              </button>
            </div>
          </div>
        ) : null}

        {/* Board Body */}
        <div className="relative z-10 flex min-h-0 flex-1 flex-col overflow-hidden">
          {loading ? (
            <div
              aria-busy="true"
              className="flex flex-1 flex-col items-center justify-center gap-3 p-8 text-content/50"
            >
              <LoaderCircle className="size-8 animate-spin text-accent" />
              <p className="text-sm">Loading project board...</p>
            </div>
          ) : totalCards === 0 && !error ? (
            <div className="flex flex-1 flex-col items-center justify-center p-8 text-center">
              <div className="mb-3 grid size-12 place-items-center rounded-2xl bg-content/5 text-content/40">
                <Plus className="size-6" />
              </div>
              <h3 className="mb-1 text-base font-semibold text-content">
                No cards on this board yet
              </h3>
              <p className="mb-4 max-w-sm text-xs text-content/55">
                Organize work with five status lanes, priorities, image attachments, and linked Mono sessions.
              </p>
              <button
                type="button"
                onClick={() => handleOpenCreate("backlog")}
                className="flex items-center gap-1.5 rounded-lg bg-accent px-4 py-2 text-xs font-medium text-white shadow-sm hover:brightness-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              >
                <Plus className="size-3.5" strokeWidth={2} />
                Create your first card
              </button>
            </div>
          ) : (
            /* Five-Lane Horizontal Scroller */
            <div
              data-lanes-container
              className="flex min-h-0 flex-1 gap-3 overflow-x-auto p-3 sm:gap-4 sm:p-5"
            >
              {BOARD_LANES.map((lane) => {
                const laneCards = cardsByLane[lane.status] ?? [];
                return (
                  <section
                    key={lane.status}
                    data-lane-status={lane.status}
                    aria-label={`${lane.label} lane, ${laneCards.length} ${laneCards.length === 1 ? "card" : "cards"}`}
                    className="flex w-[260px] sm:w-[280px] shrink-0 flex-col rounded-xl border border-content/8 bg-content/[0.02]"
                  >
                    {/* Lane Header */}
                    <div className="flex items-center justify-between border-b border-content/6 px-3 py-2.5">
                      <div className="flex items-center gap-2">
                        <h3 className="text-xs font-semibold text-content/80">
                          {lane.label}
                        </h3>
                        <span className="rounded-full bg-content/8 px-1.5 py-0.2 text-[10px] font-semibold text-content/60 tabular-nums">
                          {laneCards.length}
                        </span>
                      </div>
                      <button
                        type="button"
                        aria-label={`Add card to ${lane.label}`}
                        title={`Add card to ${lane.label}`}
                        onClick={() => handleOpenCreate(lane.status)}
                        className="grid size-6 place-items-center rounded text-content/40 hover:bg-content/8 hover:text-content focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent"
                      >
                        <Plus className="size-3.5" strokeWidth={1.75} />
                      </button>
                    </div>

                    {/* Lane Cards Scroll Area */}
                    <div className="flex min-h-0 flex-1 flex-col gap-2.5 overflow-y-auto p-2.5">
                      {laneCards.length === 0 ? (
                        <div className="grid flex-1 place-items-center rounded-lg border border-dashed border-content/6 p-4 text-center">
                          <p className="text-[11px] text-content/35">
                            No cards in {lane.label}
                          </p>
                        </div>
                      ) : (
                        laneCards.map((card) => (
                          <BoardCard
                            key={card.id}
                            card={card}
                            projectCwd={projectCwd}
                            repository={repository}
                            onEdit={setEditingCard}
                            onDelete={handleDeleteCard}
                            onMove={handleMoveCard}
                            onOpenSession={onOpenSession}
                            onMediaDeleted={handleMediaDeleted}
                          />
                        ))
                      )}
                    </div>
                  </section>
                );
              })}
            </div>
          )}
        </div>
      </div>

      {/* Create Card Modal */}
      {isCreating ? (
        <CardEditorModal
          projectCwd={projectCwd}
          defaultStatus={createStatus}
          repository={repository}
          onClose={() => setIsCreating(false)}
          onSaved={handleCardSaved}
        />
      ) : null}

      {/* Edit Card Modal */}
      {editingCard ? (
        <CardEditorModal
          projectCwd={projectCwd}
          initialCard={editingCard}
          repository={repository}
          onClose={() => setEditingCard(null)}
          onSaved={handleCardSaved}
        />
      ) : null}

      {/* Delete Confirmation Modal */}
      {pendingDeleteCard ? (
        <DeleteConfirmModal
          card={pendingDeleteCard}
          isDeleting={isDeletingCard}
          onCancel={() => setPendingDeleteCard(null)}
          onConfirm={handleConfirmDelete}
        />
      ) : null}
    </div>
  );
}

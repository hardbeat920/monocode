import { useState } from "react";
import {
  ArrowUp,
  ChevronDown,
  ExternalLink,
  ImagePlus,
  Minus,
  Pencil,
  Trash2,
} from "../../../shared/ui/icons";
import {
  BOARD_LANES,
  type BoardMediaRef,
  type BoardPriority,
  type BoardStatus,
  type ProjectBoardCard,
  type ProjectBoardRepository,
} from "./types";
import { MediaPreviewModal } from "./MediaPreviewModal";

interface Props {
  card: ProjectBoardCard;
  projectCwd: string;
  repository: ProjectBoardRepository;
  onEdit: (card: ProjectBoardCard) => void;
  onDelete: (cardId: string) => void;
  onMove: (card: ProjectBoardCard, nextStatus: BoardStatus) => void;
  onOpenSession: (sessionId: string) => void;
}

function PriorityBadge({ priority }: { priority: BoardPriority }) {
  switch (priority) {
    case "high":
      return (
        <span
          title="High priority"
          className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-rose-400 bg-rose-500/10"
        >
          <ArrowUp className="size-2.5" strokeWidth={2.5} />
          High
        </span>
      );
    case "medium":
      return (
        <span
          title="Medium priority"
          className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-amber-400 bg-amber-500/10"
        >
          <Minus className="size-2.5" strokeWidth={2.5} />
          Medium
        </span>
      );
    case "low":
    default:
      return (
        <span
          title="Low priority"
          className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-emerald-400 bg-emerald-500/10"
        >
          <ChevronDown className="size-2.5" strokeWidth={2.5} />
          Low
        </span>
      );
  }
}

export function BoardCard({
  card,
  projectCwd,
  repository,
  onEdit,
  onDelete,
  onMove,
  onOpenSession,
}: Props) {
  const [selectedPreview, setSelectedPreview] = useState<BoardMediaRef | null>(
    null,
  );

  const otherLanes = BOARD_LANES.filter((l) => l.status !== card.status);

  return (
    <article
      data-card-id={card.id}
      aria-label={`Card: ${card.title}`}
      className="group relative flex flex-col rounded-xl border border-content/8 bg-background-base p-3 shadow-xs transition-[border-color,box-shadow] hover:border-content/20 hover:shadow-md"
    >
      {/* Top row: Priority & Actions */}
      <div className="mb-2 flex items-center justify-between gap-2">
        <PriorityBadge priority={card.priority} />

        <div className="flex items-center gap-1 opacity-70 group-hover:opacity-100 transition-opacity">
          {/* Quick status lane mover */}
          <label htmlFor={`move-select-${card.id}`} className="sr-only">
            Move card {card.title} to another lane
          </label>
          <select
            id={`move-select-${card.id}`}
            aria-label={`Move card ${card.title} to lane`}
            value={card.status}
            onChange={(e) => onMove(card, e.target.value as BoardStatus)}
            className="h-6 rounded border border-content/10 bg-content/4 px-1.5 text-[11px] text-content/70 hover:bg-content/8 focus:border-accent focus:outline-none"
          >
            <option value={card.status} disabled>
              Move to...
            </option>
            {otherLanes.map((lane) => (
              <option key={lane.status} value={lane.status}>
                {lane.label}
              </option>
            ))}
          </select>

          <button
            type="button"
            aria-label={`Edit card ${card.title}`}
            title="Edit card"
            onClick={() => onEdit(card)}
            className="grid size-6 place-items-center rounded text-content/50 hover:bg-content/8 hover:text-content focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent"
          >
            <Pencil className="size-3" />
          </button>

          <button
            type="button"
            aria-label={`Delete card ${card.title}`}
            title="Delete card"
            onClick={() => onDelete(card.id)}
            className="grid size-6 place-items-center rounded text-content/50 hover:bg-red-500/10 hover:text-red-400 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-red-400"
          >
            <Trash2 className="size-3" />
          </button>
        </div>
      </div>

      {/* Card Title */}
      <h4 className="mb-1 text-sm font-semibold leading-snug text-content wrap-anywhere">
        {card.title}
      </h4>

      {/* Card Description */}
      {card.description ? (
        <p className="mb-2 text-xs leading-relaxed text-content/65 line-clamp-3 wrap-anywhere whitespace-pre-wrap">
          {card.description}
        </p>
      ) : null}

      {/* Media Attachments Preview */}
      {card.media && card.media.length > 0 ? (
        <div className="mb-2 flex flex-wrap gap-1.5">
          {card.media.map((item) => (
            <button
              key={item.id}
              type="button"
              aria-label={`View attachment ${item.name}`}
              title={`${item.name} (${Math.round(item.byteLength / 1024)} KB)`}
              onClick={() => setSelectedPreview(item)}
              className="group/img relative flex h-10 items-center gap-1 overflow-hidden rounded border border-content/10 bg-content/3 px-2 text-[11px] text-content/75 hover:border-content/25 hover:bg-content/6 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent"
            >
              <ImagePlus className="size-3.5 shrink-0 text-accent" />
              <span className="max-w-[100px] truncate">{item.name}</span>
            </button>
          ))}
        </div>
      ) : null}

      {/* Linked Sessions */}
      {card.linkedSessionIds && card.linkedSessionIds.length > 0 ? (
        <div className="mt-auto pt-2 border-t border-content/6 flex flex-wrap items-center gap-1.5">
          <span className="text-[10px] font-medium text-content/40">
            Sessions:
          </span>
          {card.linkedSessionIds.map((sessionId) => (
            <button
              key={sessionId}
              type="button"
              aria-label={`Open session ${sessionId}`}
              title={`Open session ${sessionId}`}
              onClick={() => onOpenSession(sessionId)}
              className="inline-flex items-center gap-1 rounded bg-content/5 px-1.5 py-0.5 text-[10px] font-mono text-content/75 hover:bg-accent/15 hover:text-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent"
            >
              <ExternalLink className="size-2.5" />
              <span className="max-w-[120px] truncate">
                {sessionId.length > 12
                  ? `${sessionId.slice(0, 6)}...${sessionId.slice(-4)}`
                  : sessionId}
              </span>
            </button>
          ))}
        </div>
      ) : null}

      {/* Attachment Preview Modal */}
      {selectedPreview ? (
        <MediaPreviewModal
          projectCwd={projectCwd}
          cardId={card.id}
          mediaRef={selectedPreview}
          repository={repository}
          onClose={() => setSelectedPreview(null)}
          onDelete={async () => {
            await repository.deleteMedia(projectCwd, card.id, selectedPreview.id);
            card.media = (card.media ?? []).filter((m) => m.id !== selectedPreview.id);
            setSelectedPreview(null);
          }}
        />
      ) : null}
    </article>
  );
}

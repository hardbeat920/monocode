import { Link, MessageMultiple, X } from "../../../shared/ui/icons";
import type {
  SessionContextCard,
  SessionDropChoice,
} from "../model/sessionContext";
import type { LinkedPeer } from "../model/sessionLinks";

function title(value: string) {
  return value.trim() || "Untitled session";
}

/** One attached session, in the composer or on a sent user message. */
export function SessionContextChip({
  card,
  onRemove,
}: {
  card: SessionContextCard;
  onRemove?: () => void;
}) {
  return (
    <span
      data-session-context-chip={card.id}
      title={`Session context: ${title(card.title)}`}
      className="inline-flex h-6 max-w-64 min-w-0 items-center gap-1.5 rounded-md border border-content/10 bg-content/6 pr-1 pl-2 text-[12px] text-content/80"
    >
      <MessageMultiple
        className="size-3.5 shrink-0 text-content/45"
        strokeWidth={1.75}
      />
      <span className="min-w-0 truncate">{title(card.title)}</span>
      {onRemove ? (
        <button
          type="button"
          title="Remove"
          aria-label={`Remove session ${title(card.title)} from context`}
          onClick={onRemove}
          className="grid size-4 shrink-0 place-items-center rounded text-content/40 hover:bg-content/10 hover:text-content"
        >
          <X className="size-3" strokeWidth={2} />
        </button>
      ) : null}
    </span>
  );
}

export function SessionContextChips({
  cards,
  onRemove,
  className = "",
}: {
  cards: readonly SessionContextCard[];
  onRemove?: (id: string) => void;
  className?: string;
}) {
  if (cards.length === 0) return null;
  return (
    <div className={`flex flex-wrap gap-1.5 ${className}`}>
      {cards.map((card) => (
        <SessionContextChip
          key={card.id}
          card={card}
          onRemove={onRemove ? () => onRemove(card.id) : undefined}
        />
      ))}
    </div>
  );
}

/** The sessions linked to this one, each with an unlink button. */
export function LinkedSessionsBar({
  peers,
  onUnlink,
}: {
  peers: readonly LinkedPeer[];
  onUnlink: (id: string) => void;
}) {
  if (peers.length === 0) return null;
  return (
    <div
      data-linked-sessions
      className="flex min-w-0 flex-wrap items-center gap-1.5 px-3 pt-2 text-[11px] text-content/50"
    >
      <span className="flex items-center gap-1">
        <Link className="size-3" strokeWidth={1.75} />
        Linked
      </span>
      {peers.map((peer) => (
        <span
          key={peer.id}
          title={`Linked session: ${title(peer.title)}`}
          className="inline-flex h-5 max-w-56 min-w-0 items-center gap-1 rounded-md border border-content/10 bg-content/6 pr-0.5 pl-1.5 text-content/75"
        >
          <span className="min-w-0 truncate">{title(peer.title)}</span>
          <button
            type="button"
            title="Unlink"
            aria-label={`Unlink session ${title(peer.title)}`}
            onClick={() => onUnlink(peer.id)}
            className="grid size-4 shrink-0 place-items-center rounded text-content/40 hover:bg-content/10 hover:text-content"
          >
            <X className="size-2.5" strokeWidth={2} />
          </button>
        </span>
      ))}
    </div>
  );
}

/** The two halves shown while a sidebar session is held over a composer. */
export function SessionDropOverlay({ choice }: { choice: SessionDropChoice }) {
  const half = (active: boolean) =>
    `grid place-items-center gap-1 rounded-md border text-[12px] transition-colors ${
      active
        ? "border-accent/60 bg-accent/15 text-content"
        : "border-content/10 bg-background-base/80 text-content/55"
    }`;
  return (
    <div
      data-session-drop-overlay
      className="pointer-events-none absolute inset-0 z-30 grid grid-cols-2 gap-1.5 rounded-lg bg-background-base/70 p-1.5 backdrop-blur-sm"
    >
      <div className={half(choice === "context")}>
        <span className="flex items-center gap-1.5">
          <MessageMultiple className="size-3.5" strokeWidth={1.75} />
          Add to context
        </span>
        <span className="text-[11px] text-content/45">Copy its messages once</span>
      </div>
      <div className={half(choice === "link")}>
        <span className="flex items-center gap-1.5">
          <Link className="size-3.5" strokeWidth={1.75} />
          Link sessions
        </span>
        <span className="text-[11px] text-content/45">
          Let the agents message each other
        </span>
      </div>
    </div>
  );
}

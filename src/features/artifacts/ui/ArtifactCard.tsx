import { CheckCircle, File } from "../../../shared/ui/icons";
import { artifactLabel, type ArtifactCard as Card } from "../artifacts";

/** One line under the reply: done, its title, and a chip naming its kind. */
export function ArtifactCard({
  card,
  onOpen,
}: {
  card: Card;
  onOpen?: (id: string) => void;
}) {
  const label = artifactLabel(card.kind);
  return (
    <button
      type="button"
      data-artifact-card={card.id}
      aria-label={`Open ${label.toLowerCase()} ${card.title}`}
      title={`Open ${label.toLowerCase()}`}
      onClick={() => onOpen?.(card.id)}
      disabled={!onOpen}
      className="flex w-fit max-w-sm min-w-0 items-center gap-2.5 rounded-xl border border-content/10 py-1.5 pr-1.5 pl-3 text-left font-sans transition-colors hover:border-content/20 hover:bg-content/5 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent disabled:opacity-60"
    >
      <CheckCircle
        className="size-4 shrink-0 text-emerald-400/90"
        strokeWidth={1.75}
      />
      <span className="min-w-0 truncate text-[13px] text-content">
        {card.title}
      </span>
      <span className="flex shrink-0 items-center gap-1 rounded-full border border-content/10 bg-content/5 px-2 py-0.5 text-[12px] text-content/65">
        <File className="size-3.5" strokeWidth={1.75} />
        {label}
      </span>
    </button>
  );
}

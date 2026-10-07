import { File, ExternalLink } from "../../../shared/ui/icons";
import { artifactLabel, type ArtifactCard as Card } from "../artifacts";

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
      onClick={() => onOpen?.(card.id)}
      disabled={!onOpen}
      className="flex w-full max-w-lg items-start gap-3 rounded-xl border border-content/10 bg-content/5 px-4 py-3.5 text-left font-sans hover:border-content/20 hover:bg-content/8 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent disabled:opacity-60"
    >
      <File
        className="mt-0.5 size-5 shrink-0 text-content/55"
        strokeWidth={1.5}
      />
      <span className="flex min-w-0 flex-1 flex-col gap-1">
        <span className="text-[11px] text-content/45">{label}</span>
        <span className="text-[14px] font-medium leading-snug text-content">
          {card.title}
        </span>
        <span className="mt-1 flex items-center gap-1 text-[12px] text-content/65">
          Open {label.toLowerCase()} <ExternalLink className="size-3" />
        </span>
      </span>
    </button>
  );
}

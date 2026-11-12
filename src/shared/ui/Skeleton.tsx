const WIDTHS = ["85%", "65%", "75%", "50%"];

/** Placeholder lines while content loads. */
export function Skeleton({ rows }: { rows: number }) {
  const count = Math.max(1, Math.min(Math.floor(rows) || 1, 24));
  return (
    <div aria-hidden className="flex flex-col gap-2 py-1">
      {Array.from({ length: count }, (_, index) => (
        <div
          key={index}
          className="h-3 animate-pulse rounded bg-content/8"
          style={{ width: WIDTHS[index % WIDTHS.length] }}
        />
      ))}
    </div>
  );
}

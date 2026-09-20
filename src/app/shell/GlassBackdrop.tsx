/**
 * Keep glass beside the interactive content, inside a positioned, isolated
 * frame. Neither this layer nor its ancestors should animate: backdrop-filter
 * already handles compositing without translateZ/backface-visibility hints.
 */
export function GlassBackdrop({
  className = "popover-backdrop",
}: {
  className?: string;
}) {
  return (
    <div
      aria-hidden="true"
      className={`pointer-events-none absolute inset-0 z-0 rounded-[inherit] backdrop-blur-xl ${className}`}
    />
  );
}

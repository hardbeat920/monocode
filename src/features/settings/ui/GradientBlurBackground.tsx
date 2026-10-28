import type { CSSProperties } from "react";

/** Shared image-only treatment for the chat pane and both background previews. */
export function GradientBlurBackground({
  className = "",
  style,
}: {
  className?: string;
  style?: CSSProperties;
}) {
  return (
    <div
      aria-hidden
      className={`gradient-blur-background ${className}`}
      style={style}
    >
      <span className="gradient-blur-background-soft" />
      <span className="gradient-blur-background-strong" />
    </div>
  );
}

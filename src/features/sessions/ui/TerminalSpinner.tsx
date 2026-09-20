import { useEffect, useState } from "react";

const FRAMES = [
  "⠋",
  "⠙",
  "⠹",
  "⠸",
  "⠼",
  "⠴",
  "⠦",
  "⠧",
  "⠇",
  "⠏",
] as const;

export function TerminalSpinner({
  className = "inline-block w-3.5 select-none text-center text-[11px] leading-none",
}: {
  className?: string;
}) {
  const [frame, setFrame] = useState(0);

  useEffect(() => {
    const id = window.setInterval(
      () => setFrame((n) => (n + 1) % FRAMES.length),
      80,
    );
    return () => window.clearInterval(id);
  }, []);

  return (
    <span aria-hidden className={className}>
      {FRAMES[frame]}
    </span>
  );
}

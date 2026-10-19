import { useEffect, useState } from "react";

// Matches the desktop's 600ms rest/talk cycle (300ms per frame).
const FRAME_MS = 300;

/** Flips between the rest and talk frames while `active`. */
export function useFrameToggle(active: boolean) {
  const [talking, setTalking] = useState(false);

  useEffect(() => {
    if (!active) return;
    const timer = setInterval(() => setTalking((t) => !t), FRAME_MS);
    return () => {
      clearInterval(timer);
      setTalking(false);
    };
  }, [active]);

  return active && talking;
}

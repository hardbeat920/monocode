import amp from "../assets/providers/amp.svg";
import claude from "../assets/providers/claude.png";
import codex from "../assets/providers/codex.png";
import cursor from "../assets/providers/cursor.png";
import fx from "../assets/providers/fx.svg";
import opencode from "../assets/providers/opencode.png";
import pi from "../assets/providers/pi.svg";
import type { HarnessId } from "../lib/session";

export const HARNESS_ICONS: Record<HarnessId, string> = {
  claude,
  codex,
  cursor,
  opencode,
  pi,
  fx,
  amp,
};

const FX_MARK =
  "M237.89 0C243.18 0 249.38 1.42 253.03 3.07L255.09 4.01L250.08 18.63L247.68 17.75C244.9 16.72 241.94 15.8 238.49 15.8C234.98 15.8 232.79 16.56 231.08 18.32C229.23 20.23 227.63 23.64 226.23 29.76L225.14 34.85H241.67L260.43 34.95L260.69 34.95L260.84 35.17L278.85 61.63L296.74 34.95H320.87L291.68 76.74L322.1 119.75H299.33L299.18 119.55L241.14 40.48L239.35 49.4H222.07L205.69 127.21C203.93 135.71 201.19 142.84 196.78 147.87C192.27 153.01 186.2 155.75 178.34 155.75C174.18 155.75 170.75 155.11 167.91 154.11L166.24 153.52V137.18L166.9 137.4L169.53 138.28C172.18 139.16 174.41 139.8 177.14 139.8C178.53 139.8 179.7 139.53 180.73 138.98C181.76 138.43 182.68 137.6 183.52 136.44C185.3 133.99 186.72 130.13 187.9 124.67L203.76 49.4H189.87L191.76 39.44L192.04 39.35L206.82 34.47L208.15 28.64C210.52 18.21 213.77 10.94 218.71 6.32C223.74 1.61 230.13 0 237.89 0ZM273.99 99.08L260.07 120.25H234.54L261 82.02L273.99 99.08Z";

export function HarnessIcon({
  harness,
  className = "size-3.5",
}: {
  harness: HarnessId;
  className?: string;
}) {
  if (harness === "pi") {
    return (
      <svg
        viewBox="0 0 800 800"
        fill="currentColor"
        aria-hidden
        className={`block ${className}`}
      >
        <path
          fillRule="evenodd"
          d="M165.29 165.29H517.36V400H400V517.36H282.65V634.72H165.29ZM282.65 282.65V400H400V282.65Z"
        />
        <path d="M517.36 400H634.72V634.72H517.36Z" />
      </svg>
    );
  }
  if (harness === "fx") {
    return (
      <svg
        viewBox="134.24 -32 220 220"
        fill="currentColor"
        aria-hidden
        className={`block ${className}`}
      >
        <path fillRule="evenodd" d={FX_MARK} />
      </svg>
    );
  }
  return (
    <img
      src={HARNESS_ICONS[harness]}
      alt=""
      draggable={false}
      className={`block object-contain ${className}`}
    />
  );
}

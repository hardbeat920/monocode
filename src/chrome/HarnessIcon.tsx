import type { ReactNode } from "react";
import claude from "../assets/providers/claude.svg";
import codex from "../assets/providers/codex.svg";
import cursor from "../assets/providers/cursor.svg";
import fx from "../assets/providers/fx.svg";
import grok from "../assets/providers/grok.svg";
import omp from "../assets/providers/omp.svg";
import opencode from "../assets/providers/opencode.svg";
import zai from "../assets/providers/zai.svg";
import mimo from "../assets/providers/mimo.svg";
import openrouter from "../assets/providers/openrouter.svg";
import nvidia from "../assets/providers/nvidia.svg";
import pi from "../assets/providers/pi.svg";
import type { HarnessId } from "../lib/session";

export const HARNESS_ICONS: Record<HarnessId, string> = {
  claude,
  codex,
  cursor,
  grok,
  opencode,
  zai,
  mimo,
  openrouter,
  nvidia,
  pi,
  omp,
  fx,
};

/** White marks that must follow `currentColor` so they stay visible in light mode. */
export const MONOCHROME_HARNESSES = new Set<HarnessId>([
  "cursor",
  "grok",
  "opencode",
  "zai",
  "mimo",
  "openrouter",
  "nvidia",
  "pi",
  "omp",
  "fx",
]);

function MonoIcon({
  className,
  children,
  viewBox = "-4 -4 37 37",
}: {
  className: string;
  children: ReactNode;
  viewBox?: string;
}) {
  return (
    <svg
      viewBox={viewBox}
      fill="currentColor"
      aria-hidden
      className={`block ${className}`}
    >
      {children}
    </svg>
  );
}

export function HarnessIcon({
  harness,
  className = "size-3.5",
}: {
  harness: HarnessId;
  className?: string;
}) {
  if (harness === "cursor") {
    return (
      <MonoIcon className={className}>
        <path
          fillRule="evenodd"
          d="M26.5001 6.86227L15.0931 0.161574C14.913 0.055725 14.7086 0 14.5006 0C14.2926 0 14.0882 0.055725 13.908 0.161574L2.49755 6.86227C2.34639 6.95136 2.22087 7.07929 2.13356 7.23325C2.04624 7.38722 2.00018 7.56182 2 7.73958V21.257C2 21.6195 2.19 21.9542 2.49874 22.1355L13.9069 28.8386C14.0871 28.9443 14.2914 29 14.4994 29C14.7074 29 14.9118 28.9443 15.092 28.8386L26.5013 22.1355C26.6528 22.0465 26.7787 21.9184 26.8662 21.7642C26.9538 21.61 26.9999 21.4351 27 21.257V7.74079C26.9997 7.56291 26.9535 7.38823 26.8659 7.23426C26.7784 7.08028 26.6526 6.95242 26.5013 6.86348L26.5001 6.86227ZM25.784 8.28337L14.7702 27.6954C14.6953 27.826 14.4994 27.7728 14.4994 27.6217V14.9104C14.4992 14.7854 14.4667 14.6627 14.4053 14.5545C14.3438 14.4464 14.2554 14.3565 14.1491 14.2941L3.33116 7.93776C3.2041 7.86284 3.25635 7.66224 3.40479 7.66224H25.4325C25.746 7.66224 25.9408 8.00785 25.784 8.28337Z"
        />
      </MonoIcon>
    );
  }
  if (harness === "fx") {
    return (
      <MonoIcon className={className}>
        <path d="M13.3315 0C14.3158 0 15.4694 0.264398 16.1485 0.571621L16.5318 0.746645L15.5996 3.46883L15.1531 3.30498C14.6358 3.11319 14.0851 2.94189 13.4432 2.94189C12.7901 2.94189 12.3826 3.0834 12.0644 3.41111C11.7202 3.76674 11.4225 4.40167 11.162 5.54119L10.9592 6.48892H14.0348L17.5254 6.50754H17.5738L17.6017 6.54851L20.9527 11.4752L24.2814 6.50754H28.7711L23.3399 14.2887L29 22.2969H24.7633L24.7354 22.2597L13.9362 7.53721L13.6032 9.19807H10.388L7.34024 23.686C7.01277 25.2686 6.50295 26.5962 5.68241 27.5328C4.84326 28.4898 3.71384 29 2.25138 29C1.47735 29 0.83915 28.8808 0.310727 28.6946L0 28.5848V25.5423L0.1228 25.5833L0.612151 25.7472C1.10522 25.911 1.52015 26.0302 2.0281 26.0302C2.28673 26.0302 2.50443 25.9799 2.69607 25.8775C2.88772 25.7751 3.0589 25.6205 3.21519 25.4046C3.54639 24.9484 3.8106 24.2297 4.03015 23.213L6.98114 9.19807H4.3967L4.74836 7.34356L4.80046 7.32681L7.55049 6.41817L7.79796 5.33265C8.23893 3.39063 8.84364 2.03698 9.7628 1.17676C10.6987 0.299775 11.8877 0 13.3315 0ZM20.0484 18.4483L17.4584 22.39H12.7082L17.6315 15.2718L20.0484 18.4483Z" />
      </MonoIcon>
    );
  }
  if (harness === "grok") {
    return (
      <MonoIcon className={className} viewBox="0 0 35 33">
        <path d="M13.2371 21.0407L24.3186 12.8506C24.8619 12.4491 25.6384 12.6057 25.8973 13.2294C27.2597 16.5185 26.651 20.4712 23.9403 23.1851C21.2297 25.8989 17.4581 26.4941 14.0108 25.1386L10.2449 26.8843C15.6463 30.5806 22.2053 29.6665 26.304 25.5601C29.5551 22.3051 30.562 17.8683 29.6205 13.8673L29.629 13.8758C28.2637 7.99809 29.9647 5.64871 33.449 0.844576C33.5314 0.730667 33.6139 0.616757 33.6964 0.5L29.1113 5.09055V5.07631L13.2343 21.0436" />
        <path d="M10.9503 23.0313C7.07343 19.3235 7.74185 13.5853 11.0498 10.2763C13.4959 7.82722 17.5036 6.82767 21.0021 8.2971L24.7595 6.55998C24.0826 6.07017 23.215 5.54334 22.2195 5.17313C17.7198 3.31926 12.3326 4.24192 8.67479 7.90126C5.15635 11.4239 4.0499 16.8403 5.94992 21.4622C7.36924 24.9165 5.04257 27.3598 2.69884 29.826C1.86829 30.7002 1.0349 31.5745 0.36364 32.5L10.9474 23.0341" />
      </MonoIcon>
    );
  }
  if (harness === "pi") {
    return (
      <MonoIcon className={className}>
        <path
          fillRule="evenodd"
          d="M1 1H22V14.4997H14.9998V21.2499H8.0002V28H1V1ZM8.0002 7.75014V14.4997H14.9998V7.75014H8.0002Z"
        />
        <path d="M22 15H28V28H22V15Z" />
      </MonoIcon>
    );
  }
  if (harness === "omp") {
    return (
      <MonoIcon className={className}>
        <path d="M1 1H29V6.5H1V1Z" />
        <path d="M5.5 6.5H10.5V28H5.5V6.5Z" />
        <path d="M18.5 6.5H23.5V17H18.5V6.5Z" />
        <path
          fillRule="evenodd"
          d="M15.5 17H26.5V25.5H15.5V17ZM19 20V22.5H20.5V20H19ZM22.5 20V22.5H24V20H22.5Z"
        />
      </MonoIcon>
    );
  }
  if (MONOCHROME_HARNESSES.has(harness)) {
    const icon = HARNESS_ICONS[harness];
    return (
      <span
        aria-hidden
        className={`block shrink-0 bg-current ${className}`}
        style={{
          maskImage: `url(${icon})`,
          maskRepeat: "no-repeat",
          maskPosition: "center",
          maskSize: "contain",
          WebkitMaskImage: `url(${icon})`,
          WebkitMaskRepeat: "no-repeat",
          WebkitMaskPosition: "center",
          WebkitMaskSize: "contain",
        }}
      />
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

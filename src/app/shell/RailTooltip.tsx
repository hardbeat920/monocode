import {
  useRef,
  useState,
  type FocusEvent,
  type PointerEvent,
  type ReactNode,
} from "react";
import { formatInteger } from "../../shared/lib/numbers";
import type { PopoverSide } from "../../shared/lib/popover";
import { Popover } from "../../shared/ui/Popover";

type Tip = {
  el: HTMLElement;
  label: string;
  detail: string | null;
  additions: number;
  deletions: number;
};

/** Any rail control carrying this attribute gets an instant tooltip. */
const TIP = "[data-rail-tip]";

function tipFor(target: EventTarget | null): Tip | null {
  const el =
    target instanceof Element ? target.closest<HTMLElement>(TIP) : null;
  const label = el?.dataset.railTip;
  if (!el || !label) return null;
  return {
    el,
    label,
    detail: el.dataset.railTipDetail ?? null,
    additions: Number(el.dataset.railTipAdditions ?? 0),
    deletions: Number(el.dataset.railTipDeletions ?? 0),
  };
}

/**
 * One tooltip for the whole icon rail, wired by delegation so each control
 * only has to declare `data-rail-tip` (and optionally `data-rail-tip-detail`,
 * or `data-rail-tip-additions` / `-deletions` for a colored diff stat).
 * Shows on hover with no delay, or on keyboard focus, and steps aside once a
 * control is clicked until the pointer moves to another.
 */
export function useRailTooltip(side: PopoverSide = "right"): {
  handlers: {
    onPointerOver: (event: PointerEvent) => void;
    onPointerLeave: () => void;
    onPointerDown: () => void;
    onFocus: (event: FocusEvent) => void;
    onBlur: () => void;
  };
  tooltip: ReactNode;
} {
  const [tip, setTip] = useState<Tip | null>(null);
  const clicked = useRef<HTMLElement | null>(null);

  const show = (next: Tip | null) => {
    if (next?.el !== clicked.current) clicked.current = null;
    if (
      next &&
      (next.el === clicked.current || next.el.ariaExpanded === "true")
    ) {
      next = null;
    }
    setTip((prev) =>
      prev?.el === next?.el &&
      prev?.label === next?.label &&
      prev?.detail === next?.detail &&
      prev?.additions === next?.additions &&
      prev?.deletions === next?.deletions
        ? prev
        : next,
    );
  };

  const handlers = {
    onPointerOver: (event: PointerEvent) => show(tipFor(event.target)),
    onPointerLeave: () => {
      clicked.current = null;
      setTip(null);
    },
    onPointerDown: () => {
      clicked.current = tip?.el ?? null;
      setTip(null);
    },
    onFocus: (event: FocusEvent) => {
      if (
        event.target instanceof Element &&
        event.target.matches(":focus-visible")
      ) {
        show(tipFor(event.target));
      }
    },
    onBlur: () => setTip(null),
  };

  const tooltip = tip ? (
    <Popover
      anchor={tip.el}
      side={side}
      align="center"
      gap={8}
      constrainHeight={false}
      rounded="rounded-lg"
      role="tooltip"
      data-rail-tooltip
      className="pointer-events-none whitespace-nowrap px-2.5 py-1.5 text-[12px] leading-tight"
    >
      <span className="font-medium text-content">{tip.label}</span>
      {tip.detail ? (
        <span className="ml-1.5 text-content/55">{tip.detail}</span>
      ) : null}
      {tip.additions > 0 ? (
        <span className="ml-1.5 font-semibold tabular-nums text-diff-add-fg">
          +{formatInteger(tip.additions)}
        </span>
      ) : null}
      {tip.deletions > 0 ? (
        <span className="ml-1.5 font-semibold tabular-nums text-diff-del-fg">
          -{formatInteger(tip.deletions)}
        </span>
      ) : null}
    </Popover>
  ) : null;

  return { handlers, tooltip };
}

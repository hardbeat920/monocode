import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { RotateCcw } from "./icons";

export type EffortTier = {
  value: string;
  label: string;
  kind: "auto" | "level" | "beyond";
};

// Option order varies by harness (Grok lists xhigh first), so the meter
// ranks by name rather than trusting catalog order. Unknown values keep
// their catalog order after the named ones.
const EFFORT_RANK: Record<string, number> = {
  auto: 0,
  off: 1,
  none: 1,
  minimal: 2,
  low: 3,
  medium: 4,
  high: 5,
  xhigh: 6,
  max: 7,
  ultracode: 8,
  ultrathink: 9,
};

const BEYOND_VALUES = new Set(["ultracode", "ultrathink"]);

export function orderEffortOptions(
  options: { value: string; label: string }[],
): EffortTier[] {
  return options
    .map((option, index) => ({ option, index }))
    .sort(
      (a, b) =>
        (EFFORT_RANK[a.option.value] ?? Number.MAX_SAFE_INTEGER) -
          (EFFORT_RANK[b.option.value] ?? Number.MAX_SAFE_INTEGER) ||
        a.index - b.index,
    )
    .map(({ option }) => ({
      value: option.value,
      label: option.label,
      kind:
        option.value === "auto"
          ? "auto"
          : BEYOND_VALUES.has(option.value)
            ? "beyond"
            : "level",
    }));
}

const THUMB = 27;
const INSET = THUMB / 2;
const position = (frac: number) =>
  `calc(${INSET}px + ${frac} * (100% - ${THUMB}px))`;
const clamp = (value: number) => Math.min(1, Math.max(0, value));

// Invert the x coordinate of cubic-bezier(.22, .75, .18, 1).
function settleEase(progress: number) {
  const bezier = (t: number, a: number, b: number) =>
    3 * (1 - t) ** 2 * t * a + 3 * (1 - t) * t * t * b + t ** 3;
  let low = 0;
  let high = 1;
  for (let i = 0; i < 16; i++) {
    const mid = (low + high) / 2;
    if (bezier(mid, 0.22, 0.18) < progress) low = mid;
    else high = mid;
  }
  return bezier((low + high) / 2, 0.75, 1);
}

// Ten irregular sites per third; spacing is at least 4% in the 2D field.
// Twenty 1px, eight 1.5px, two 2px. Larger particles stay dimmer.
const FULL_SPECKS = [
  [2, 57],
  [5, 26],
  [9, 73],
  [14, 41],
  [17, 64],
  [21, 23],
  [24, 49],
  [27, 79],
  [30, 34],
  [32, 61],
  [35, 21],
  [38, 68],
  [42, 43],
  [45, 77],
  [48, 29],
  [51, 56],
  [55, 22],
  [58, 71],
  [61, 38],
  [65, 59],
  [68, 79],
  [71, 31],
  [74, 54],
  [78, 23],
  [81, 67],
  [84, 42],
  [88, 76],
  [91, 27],
  [95, 53],
  [98, 72],
].map(([left, top], index) => {
  const size =
    index === 9 || index === 24
      ? 2
      : [1, 5, 11, 14, 18, 21, 26, 28].includes(index)
        ? 1.5
        : 1;
  const base =
    size === 2 ? 0.22 : size === 1.5 ? 0.27 : 0.32 + (index % 5) * 0.02;
  const duration = (
    { 3: 6400, 12: 7300, 20: 8100, 27: 9200 } as Record<number, number>
  )[index];
  return { left, top, size, base, duration };
});

function renderSpeck(
  site: (typeof FULL_SPECKS)[number],
  key: number,
  isHighlight = false,
) {
  return (
    <i
      key={key}
      className="effort-speck"
      data-glow={site.size === 2 ? "" : undefined}
      data-twinkle={!isHighlight && site.duration ? "" : undefined}
      style={{
        left: `${site.left}%`,
        top: `${site.top}%`,
        width: `${site.size}px`,
        height: `${site.size}px`,
      }}
    >
      <i
        className="effort-speck-inner"
        style={
          {
            "--speck-base": site.base,
            "--speck-peak": site.base + 0.06,
            "--speck-duration": `${site.duration}ms`,
            "--speck-delay": `${-key * 317}ms`,
          } as CSSProperties
        }
      />
    </i>
  );
}

/**
 * The rail only: a continuous track that fills to the selected tier, with a
 * tick mark per tier, stationary speck field, separate thumb, and top-tier accent aura.
 */
export function EffortMeterSpark({
  tiers,
  selectedIndex,
  size,
  className,
  dragFrac,
}: {
  tiers: EffortTier[];
  selectedIndex: number;
  size: "mini" | "full";
  className?: string;
  dragFrac?: number | null;
}) {
  const n = tiers.length;
  if (n === 0) return null;
  const tierFrac = n > 1 ? selectedIndex / (n - 1) : 0;
  const frac = dragFrac != null ? dragFrac : tierFrac;
  const topTier = selectedIndex === n - 1;

  if (size === "mini") {
    return (
      <div
        aria-hidden="true"
        data-effort-rail="mini"
        className={`effort-rail effort-rail-mini${className ? ` ${className}` : ""}`}
        style={
          {
            "--effort-frac": `${frac * 100}%`,
            "--effort-t": frac,
          } as CSSProperties
        }
      >
        <div
          className="effort-rail-fill"
          data-top-tier={topTier ? "" : undefined}
          style={{ width: `${frac * 100}%` }}
        />
        <div className="effort-specks">
          <i
            className="effort-speck"
            style={{ left: "45%", top: "50%", opacity: 0.28 }}
          />
          <i
            className="effort-speck"
            style={{ left: "70%", top: "35%", opacity: 0.38 }}
          />
          <i
            className="effort-speck"
            style={{ left: "88%", top: "60%", opacity: 0.48 }}
          />
        </div>
      </div>
    );
  }

  return (
    <div className={`relative w-full${className ? ` ${className}` : ""}`}>
      <div
        aria-hidden="true"
        data-effort-rail="full"
        className="effort-rail"
        style={
          {
            "--effort-frac": position(frac),
            "--effort-thumb": `${THUMB}px`,
            "--effort-t": frac,
          } as CSSProperties
        }
      >
        <div className="effort-rail-fill">
          <div
            className="effort-top-tint"
            data-top-tier={topTier ? "" : undefined}
          />
        </div>
        <div className="effort-specks">
          {FULL_SPECKS.map((site, i) => renderSpeck(site, i))}
        </div>
        <div className="effort-specks-highlight" aria-hidden="true">
          {FULL_SPECKS.map((site, i) => renderSpeck(site, i, true))}
        </div>
        <div className="effort-ticks">
          {tiers.map((tier, index) => (
            <i
              key={tier.value}
              className="effort-rail-tick"
              data-passed={n > 1 && index / (n - 1) < frac ? "" : undefined}
              data-auto={tier.kind === "auto" ? "" : undefined}
              style={{
                left: position(n > 1 ? index / (n - 1) : 0),
              }}
            />
          ))}
        </div>
        <div className="effort-thumb">
          <div className="effort-thumb-inner" />
        </div>
      </div>
    </div>
  );
}

/**
 * Discrete effort control: a glowing rail that acts as a slider. The thumb
 * tracks continuous drag and snaps between tier ticks, stationary specks
 * twinkle inside the fill, and the top tier takes the accent halo and tint.
 */
export function EffortMeter({
  tiers,
  value,
  defaultValue,
  modelName,
  onChange,
  onClose,
}: {
  tiers: EffortTier[];
  value: string;
  defaultValue: string;
  modelName: string;
  onChange: (value: string) => void;
  onClose: () => void;
}) {
  const sliderRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  const pointerId = useRef<number | null>(null);
  const grabOffset = useRef(0);
  const pointerX = useRef(0);
  const frame = useRef<number | null>(null);
  const velocity = useRef(0);
  const lastSample = useRef({ x: 0, time: 0 });
  const [isPressed, setIsPressed] = useState(false);

  const selectedIndex = Math.max(
    0,
    tiers.findIndex((tier) => tier.value === value),
  );
  const committedFrac =
    tiers.length > 1 ? selectedIndex / (tiers.length - 1) : 0;
  const [renderedFrac, setRenderedFrac] = useState(committedFrac);
  const rendered = useRef(committedFrac);
  const settleTarget = useRef<number | null>(null);
  const [motion, setMotion] = useState({
    light: 0,
    sx: 1,
    sy: 1,
    origin: "50%",
  });
  const [settleMs, setSettleMs] = useState(140);
  const paint = (frac: number) => {
    rendered.current = frac;
    setRenderedFrac(frac);
  };
  const stopFrame = () => {
    if (frame.current != null) cancelAnimationFrame(frame.current);
    frame.current = null;
  };
  const railRect = () =>
    sliderRef.current?.querySelector(".effort-rail")?.getBoundingClientRect();
  const settle = (target: number, duration: number) => {
    stopFrame();
    settleTarget.current = target;
    const from = rendered.current;
    const distance =
      Math.abs(target - from) * Math.max(0, (railRect()?.width ?? 0) - THUMB);
    const reduced = window.matchMedia?.(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    const ms = distance < 0.5 || reduced ? 0 : duration;
    setSettleMs(ms);
    if (!ms) {
      paint(target);
      return;
    }
    const start = performance.now();
    const tick = (now: number) => {
      const t = clamp((now - start) / ms);
      paint(t === 1 ? target : from + (target - from) * settleEase(t));
      frame.current = t < 1 ? requestAnimationFrame(tick) : null;
    };
    frame.current = requestAnimationFrame(tick);
  };
  useLayoutEffect(() => {
    if (!dragging.current && settleTarget.current !== committedFrac) {
      settle(committedFrac, 140);
    }
    // Only committed changes initiate keyboard/reset settling.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [committedFrac]);
  useEffect(() => () => stopFrame(), []);

  const shownIndex = isPressed
    ? Math.round(renderedFrac * Math.max(0, tiers.length - 1))
    : selectedIndex;
  const shownTier = tiers[shownIndex];
  const isDefault = value === defaultValue;
  const defaultLabel =
    tiers.find((tier) => tier.value === defaultValue)?.label ?? defaultValue;

  useEffect(() => {
    sliderRef.current?.focus();
  }, []);

  const fracAt = (clientX: number) => {
    const rect = railRect();
    if (!rect || rect.width <= THUMB) return 0;
    return clamp(
      (clientX - grabOffset.current - rect.left - INSET) / (rect.width - THUMB),
    );
  };
  const sampleDrag = (now: number) => {
    const frac = fracAt(pointerX.current);
    const x = pointerX.current;
    const dt = Math.max(1, now - lastSample.current.time);
    const instantaneous = (x - lastSample.current.x) / dt;
    velocity.current +=
      (instantaneous - velocity.current) * (1 - Math.exp(-dt / 45));
    lastSample.current = { x, time: now };
    const s = clamp(Math.abs(velocity.current) / 0.65);
    setMotion({
      light: 0.55 + 0.45 * s,
      sx: 0.985 + 0.03 * s,
      sy: 0.985 - 0.015 * s,
      origin:
        velocity.current > 0 ? "45%" : velocity.current < 0 ? "55%" : "50%",
    });
    paint(frac);
    frame.current = requestAnimationFrame(sampleDrag);
  };
  const finishDrag = (cancel: boolean, clientX = pointerX.current) => {
    if (!dragging.current) return;
    const frac = fracAt(clientX);
    const index = Math.round(frac * Math.max(0, tiers.length - 1));
    const target = cancel
      ? committedFrac
      : tiers.length > 1
        ? index / (tiers.length - 1)
        : 0;
    dragging.current = false;
    pointerId.current = null;
    setIsPressed(false);
    setMotion({ light: 0, sx: 1, sy: 1, origin: "50%" });
    const distance =
      Math.abs(target - rendered.current) *
      Math.max(0, (railRect()?.width ?? 0) - THUMB);
    settle(target, Math.min(190, Math.max(110, 110 + 2 * distance)));
    if (!cancel) commit(index);
  };

  const commit = (index: number) => {
    const clamped = Math.min(tiers.length - 1, Math.max(0, index));
    const tier = tiers[clamped];
    if (tier && clamped !== selectedIndex) onChange(tier.value);
  };

  // React attaches wheel listeners passively, so the step-on-scroll handler
  // has to be native for preventDefault to stick.
  const stepRef = useRef((_delta: number) => {});
  stepRef.current = (delta) => commit(selectedIndex + delta);
  useEffect(() => {
    const el = sliderRef.current;
    if (!el) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      if (event.deltaY !== 0) stepRef.current(event.deltaY < 0 ? 1 : -1);
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

  if (!shownTier) return null;

  return (
    <div className="px-4 pt-3 pb-3">
      <div className="flex h-6 items-center justify-between gap-2">
        <span className="min-w-0 truncate text-[13px] font-medium leading-[18px] text-content/92">
          {shownTier.label}
        </span>
        <button
          type="button"
          aria-label="Reset to default"
          title={`Reset to ${defaultLabel}`}
          onClick={() => onChange(defaultValue)}
          tabIndex={isDefault ? -1 : 0}
          style={{ visibility: isDefault ? "hidden" : "visible" }}
          className="grid size-6 shrink-0 place-items-center rounded-[7px] text-content/48 transition-colors duration-100 hover:bg-content/6 hover:text-content/82"
        >
          <RotateCcw className="size-3.5" strokeWidth={1.5} />
        </button>
      </div>
      <div className="mt-0.5 truncate text-[11px] font-normal leading-4 text-content/54">
        {modelName}
      </div>
      <div
        ref={sliderRef}
        role="slider"
        tabIndex={0}
        aria-label="Effort"
        aria-valuemin={0}
        aria-valuemax={tiers.length - 1}
        aria-valuenow={shownIndex}
        aria-valuetext={shownTier.label}
        aria-orientation="horizontal"
        onKeyDown={(event) => {
          const last = tiers.length - 1;
          let next: number | null = null;
          if (event.key === "ArrowRight" || event.key === "ArrowUp") {
            next = selectedIndex + 1;
          } else if (event.key === "ArrowLeft" || event.key === "ArrowDown") {
            next = selectedIndex - 1;
          } else if (event.key === "Home") {
            next = 0;
          } else if (event.key === "End") {
            next = last;
          } else if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            onClose();
            return;
          }
          if (next == null) return;
          event.preventDefault();
          commit(next);
        }}
        onPointerDown={(event) => {
          if (dragging.current || event.button !== 0) return;
          event.preventDefault();
          stopFrame();
          settleTarget.current = null;
          const rect = railRect();
          const center =
            (rect?.left ?? 0) +
            INSET +
            rendered.current * Math.max(0, (rect?.width ?? 0) - THUMB);
          const offset = event.clientX - center;
          const dy =
            event.clientY - ((rect?.top ?? 0) + (rect?.height ?? 22) / 2);
          grabOffset.current = Math.hypot(offset, dy) <= INSET ? offset : 0;
          pointerX.current = event.clientX;
          pointerId.current = event.pointerId;
          sliderRef.current?.setPointerCapture?.(event.pointerId);
          sliderRef.current?.focus();
          dragging.current = true;
          velocity.current = 0;
          lastSample.current = {
            x: event.clientX,
            time: performance.now(),
          };
          setIsPressed(true);
          setMotion({ light: 0.55, sx: 0.985, sy: 0.985, origin: "50%" });
          frame.current = requestAnimationFrame(sampleDrag);
        }}
        onPointerMove={(event) => {
          if (event.pointerId === pointerId.current)
            pointerX.current = event.clientX;
        }}
        onPointerUp={(event) => {
          if (event.pointerId === pointerId.current)
            finishDrag(false, event.clientX);
        }}
        onPointerCancel={(event) => {
          if (event.pointerId === pointerId.current) finishDrag(true);
        }}
        onLostPointerCapture={(event) => {
          if (event.pointerId === pointerId.current) finishDrag(true);
        }}
        data-dragging={isPressed ? "" : undefined}
        data-sampled=""
        data-pressed={isPressed ? "" : undefined}
        className="effort-slider relative mt-2 flex h-10 w-full items-center select-none"
        style={
          {
            touchAction: "none",
            "--effort-light": motion.light,
            "--effort-sx": motion.sx,
            "--effort-sy": motion.sy,
            "--effort-origin": motion.origin,
            "--effort-settle-ms": `${settleMs}ms`,
          } as CSSProperties
        }
      >
        <EffortMeterSpark
          tiers={tiers}
          selectedIndex={shownIndex}
          size="full"
          dragFrac={renderedFrac}
        />
      </div>
      <div className="flex items-center justify-between font-sans text-[10px] font-normal leading-[14px] text-content/48">
        <span>{tiers[0]?.label}</span>
        <span>{tiers[tiers.length - 1]?.label}</span>
      </div>
    </div>
  );
}

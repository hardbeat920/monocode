import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { hexToHsv, hsvToHex, normalizeHex, type Hsv } from "../lib/colorUtils";
import { Pipette } from "./icons";

type Props = {
  value: string;
  onChange: (hex: string) => void;
};

export function ColorSwatchRow({
  colors,
  colorIndex,
  customColor,
  customPickerOpen,
  customHighlighted,
  onPickIndex,
  onToggleCustom,
}: {
  colors: readonly string[];
  colorIndex: number | null | undefined;
  customColor: string | null | undefined;
  customPickerOpen: boolean;
  customHighlighted?: boolean;
  onPickIndex: (index: number) => void;
  onToggleCustom?: () => void;
}) {
  const pipetteActive = customHighlighted ?? (customColor != null || customPickerOpen);
  return (
    <div className="flex items-center justify-between gap-1 px-0.5">
      {colors.map((color, index) => {
        const selected =
          customColor == null &&
          (colorIndex === index || (colorIndex == null && index === 0));
        return (
          <button
            key={color}
            type="button"
            title={`Color ${index + 1}`}
            aria-label={`Color ${index + 1}`}
            aria-pressed={selected}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => onPickIndex(index)}
            className="grid size-5 place-items-center rounded-full"
          >
            <span
              className={`size-3.5 rounded-full ${
                selected
                  ? "ring-2 ring-content/80 ring-offset-1 ring-offset-transparent"
                  : ""
              }`}
              style={{ background: color }}
            />
          </button>
        );
      })}
      <button
        type="button"
        title="Custom color"
        aria-label="Custom color"
        aria-expanded={customPickerOpen}
        aria-pressed={customColor != null}
        onMouseDown={(event) => event.preventDefault()}
        onClick={onToggleCustom}
        className="grid size-5 place-items-center rounded-full"
      >
        <span
          className={`grid size-3.5 place-items-center overflow-hidden rounded-full ${
            pipetteActive
              ? "ring-2 ring-content/80 ring-offset-1 ring-offset-transparent"
              : ""
          }`}
          style={
            customColor
              ? { background: customColor }
              : {
                  background:
                    "conic-gradient(red, yellow, lime, aqua, blue, magenta, red)",
                }
          }
        >
          {!customColor ? (
            <Pipette className="size-2 text-white drop-shadow-sm" strokeWidth={2.25} />
          ) : null}
        </span>
      </button>
    </div>
  );
}

export function ColorPickerPopover({ value, onChange }: Props) {
  const [hsv, setHsv] = useState<Hsv>(() => hexToHsv(value));
  const svRef = useRef<HTMLDivElement>(null);
  const hueRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const hex = normalizeHex(value);
    setHsv((prev) =>
      hsvToHex(prev.h, prev.s, prev.v) === hex ? prev : hexToHsv(hex),
    );
  }, [value]);

  const applyHsv = useCallback(
    (updater: Hsv | ((prev: Hsv) => Hsv)) => {
      setHsv((prev) => {
        const next = typeof updater === "function" ? updater(prev) : updater;
        onChange(hsvToHex(next.h, next.s, next.v));
        return next;
      });
    },
    [onChange],
  );

  const onHexInput = (raw: string) => {
    const trimmed = raw.trim();
    if (!/^#?[0-9a-fA-F]{6}$/.test(trimmed)) return;
    const hex = trimmed.startsWith("#") ? trimmed : `#${trimmed}`;
    const next = hexToHsv(hex);
    setHsv(next);
    onChange(normalizeHex(hex));
  };

  const onSvPointer = (event: ReactPointerEvent<HTMLDivElement>) => {
    const el = svRef.current;
    if (!el) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);

    const update = (clientX: number, clientY: number) => {
      const rect = el.getBoundingClientRect();
      const s = clamp01((clientX - rect.left) / rect.width) * 100;
      const v = (1 - clamp01((clientY - rect.top) / rect.height)) * 100;
      applyHsv((prev) => ({ ...prev, s, v }));
    };

    update(event.clientX, event.clientY);
    const onMove = (e: PointerEvent) => update(e.clientX, e.clientY);
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
  };

  const onHuePointer = (event: ReactPointerEvent<HTMLDivElement>) => {
    const el = hueRef.current;
    if (!el) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);

    const update = (clientX: number) => {
      const rect = el.getBoundingClientRect();
      const h = clamp01((clientX - rect.left) / rect.width) * 360;
      applyHsv((prev) => ({ ...prev, h }));
    };

    update(event.clientX);
    const onMove = (e: PointerEvent) => update(e.clientX);
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
  };

  const preview = hsvToHex(hsv.h, hsv.s, hsv.v);
  const hueColor = hsvToHex(hsv.h, 100, 100);

  return (
    <div className="mt-2 rounded-lg border border-content/10 bg-content/5 p-2">
      <div
        ref={svRef}
        role="slider"
        aria-label="Saturation and brightness"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(hsv.s)}
        className="relative h-28 w-full cursor-crosshair touch-none rounded-md"
        style={{
          background: `linear-gradient(to top, #000, transparent), linear-gradient(to right, #fff, ${hueColor})`,
        }}
        onPointerDown={onSvPointer}
      >
        <span
          className="pointer-events-none absolute size-3.5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white shadow-md"
          style={{
            left: `${hsv.s}%`,
            top: `${100 - hsv.v}%`,
            background: preview,
          }}
        />
      </div>

      <div
        ref={hueRef}
        role="slider"
        aria-label="Hue"
        aria-valuemin={0}
        aria-valuemax={360}
        aria-valuenow={Math.round(hsv.h)}
        className="relative mt-2 h-3 w-full cursor-ew-resize touch-none rounded-full"
        style={{
          background:
            "linear-gradient(to right, #f00, #ff0, #0f0, #0ff, #00f, #f0f, #f00)",
        }}
        onPointerDown={onHuePointer}
      >
        <span
          className="pointer-events-none absolute top-1/2 size-3.5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white shadow-md"
          style={{
            left: `${(hsv.h / 360) * 100}%`,
            background: hueColor,
          }}
        />
      </div>

      <div className="mt-2 flex items-center gap-2">
        <span
          className="size-7 shrink-0 rounded-md border border-content/10"
          style={{ background: preview }}
          aria-hidden
        />
        <input
          type="text"
          value={preview}
          spellCheck={false}
          aria-label="Hex color"
          onChange={(e) => onHexInput(e.target.value)}
          className="min-w-0 flex-1 rounded-md border border-content/10 bg-content/5 px-2 py-1 font-mono text-[12px] text-content outline-none ring-accent/40 focus:ring-1"
        />
      </div>
    </div>
  );
}

function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value));
}

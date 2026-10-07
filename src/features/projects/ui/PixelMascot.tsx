import type { CSSProperties } from "react";
import {
  BULB,
  PIXEL_GRID,
  pixelLayers,
  type PixelLayers,
  type PixelRect,
} from "../model/pixelMascots";

/** What a mascot's agent is doing: idle bobs, working hops, waiting gets an idea. */
export type PixelMascotStatus = "idle" | "working" | "needs-you";

type Props = {
  /** Project mascot name; see `projectMascots`. */
  name: string;
  /** Palette color, `hsl(H S% L%)` or `#rrggbb`. */
  color: string;
  status?: PixelMascotStatus;
  /** Skips idle bobbing and blinking, for mascots repeated down a list. */
  still?: boolean;
  className?: string;
};

/** Stable blink offset so mascots side by side don't blink in unison. */
function blinkDelay(seed: string): string {
  let hash = 0;
  for (let i = 0; i < seed.length; i++) {
    hash = (hash * 31 + seed.charCodeAt(i)) >>> 0;
  }
  return `-${(hash % 5200) / 1000}s`;
}

/** Frames a pixel figure on its 16×16 grid. */
const PIXEL_VIEW_BOX = `0 0 ${PIXEL_GRID} ${PIXEL_GRID}`;

/** Shaded pixel mascot, the high-fidelity take on `ProjectMascot`. */
export function PixelMascot({
  name,
  color,
  status = "idle",
  still = false,
  className = "size-7 shrink-0",
}: Props) {
  return (
    <svg
      aria-hidden
      viewBox={PIXEL_VIEW_BOX}
      overflow="visible"
      shapeRendering="crispEdges"
      className={`pixel-mascot ${className}`}
    >
      <PixelArt name={name} color={color} status={status} still={still} />
    </svg>
  );
}

function Rects({ rects }: { rects: readonly PixelRect[] }) {
  return (
    <>
      {rects.map(({ x, y, w, h, fill }, i) => (
        <rect key={i} x={x} y={y} width={w} height={h} fill={fill} />
      ))}
    </>
  );
}

/** One frame: the body, with eyes that blink unless `still`. */
function Frame({
  layers,
  still,
  blink,
}: {
  layers: PixelLayers;
  still: boolean;
  blink: CSSProperties;
}) {
  return (
    <>
      <Rects rects={layers.body} />
      <g className={still ? undefined : "pixel-eyes-open"} style={blink}>
        <Rects rects={layers.open} />
      </g>
      {still ? null : (
        <g className="pixel-eyes-closed" style={blink}>
          <Rects rects={layers.closed} />
        </g>
      )}
    </>
  );
}

function PixelArt({
  name,
  color,
  status,
  still,
}: {
  name: string;
  color: string;
  status: PixelMascotStatus;
  still: boolean;
}) {
  const working = status === "working";
  const waiting = status === "needs-you";
  const rest = pixelLayers(name, color);
  const talk = working ? pixelLayers(name, color, true) : null;
  const blink = { animationDelay: blinkDelay(name + color) } as CSSProperties;

  return (
    <>
      <g
        className={
          working ? "pixel-hop" : still || waiting ? undefined : "pixel-bob"
        }
      >
        {talk ? (
          <>
            <g className="pixel-frame-a">
              <Frame layers={rest} still={still} blink={blink} />
            </g>
            <g className="pixel-frame-b">
              <Frame layers={talk} still={still} blink={blink} />
            </g>
          </>
        ) : (
          <Frame layers={rest} still={still} blink={blink} />
        )}
      </g>

      {waiting ? (
        <g className="pixel-bulb" transform="translate(12.5 -2.5)">
          <Rects rects={BULB} />
        </g>
      ) : null}
    </>
  );
}

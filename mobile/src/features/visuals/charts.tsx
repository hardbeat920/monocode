// The desktop's dithered charts (crates/monocode-gpui/src/visuals/charts.rs),
// drawn with SVG: the same cells, tones and curves, without its hover
// readouts and draw-in motion.
import { useState, type ReactNode } from "react";
import { type LayoutChangeEvent, StyleSheet, Text, View } from "react-native";
import Svg, { G, Path, Rect, Text as SvgText } from "react-native-svg";

import { useColors, useScheme, withAlpha } from "@/ui/theme";

import { CELL, center, cells, inked, step } from "./dither";
import { compact, Curve, Scale } from "./scale";
import type { Series, Slice, ThemeColor } from "./types";

/** Desktop series hues, tuned for dark and for light surfaces. */
const HUES: Partial<Record<ThemeColor, [string, string]>> = {
  positive: ["#4fd57f", "#0a924b"],
  negative: ["#fc5855", "#d01d21"],
  warning: ["#fea247", "#d96700"],
  series2: ["#4fd57f", "#0a924b"],
  series3: ["#b77df8", "#863ad2"],
  series4: ["#fea247", "#d96700"],
  series5: ["#fa78b2", "#d12c80"],
  series6: ["#3faff3", "#0077d7"],
};

/** A theme colour token as the desktop resolves it. */
export function useVisualColor() {
  const colors = useColors();
  const dark = useScheme() === "dark";
  return (token: ThemeColor | undefined = "accent") => {
    if (token === "ink") return colors.content;
    if (token === "muted") return withAlpha(colors.content, 0.6);
    if (token === "faint") return withAlpha(colors.content, 0.2);
    const hue = HUES[token];
    return hue ? hue[dark ? 0 : 1] : colors.accent;
  };
}

/** Measures its width, then draws `children` for it. */
export function Measured({ height, children }: { height: number; children: (width: number) => ReactNode }) {
  const [width, setWidth] = useState(0);
  return (
    <View
      style={{ height, width: "100%" }}
      onLayout={(event: LayoutChangeEvent) => setWidth(Math.floor(event.nativeEvent.layout.width))}
    >
      {width > 0 ? children(width) : null}
    </View>
  );
}

/** One `<Path>` per colour of a cell grid, offset to (x, y). */
function Cells({ paths, x = 0, y = 0 }: { paths: Map<string, string>; x?: number; y?: number }) {
  return (
    <>
      {[...paths].map(([color, d]) => (
        <Path key={color} d={d} fill={color} x={x} y={y} />
      ))}
    </>
  );
}

function useAxis() {
  const colors = useColors();
  return { fill: withAlpha(colors.content, 0.45), fontSize: 10, fontFamily: "Menlo" } as const;
}

const BAR_LEVEL = 0.75;
const BAR_FLOOR = 0.125;
const BAR_FADE = 0.7;
const MAX_BAR = 36;
const AREA_LEVEL = 0.625;
const AREA_BANDS: [number, number][] = [
  [0.05, 1],
  [0.13, 0.8],
  [0.25, 0.65],
  [0.4, 0.5],
  [0.6, 0.35],
];
const AREA_REST = 0.2;

/** Bars, lines or areas over a ticked axis, as the desktop's cartesian chart. */
export function Cartesian({
  kind,
  series,
  labels,
  height,
}: {
  kind: "bar" | "line" | "area";
  series: Series[];
  labels: string[];
  height: number;
}) {
  const color = useVisualColor();
  const colors = useColors();
  const axis = useAxis();
  const dark = useScheme() === "dark";
  return (
    <Measured height={height}>
      {(width) => {
        const bars = kind === "bar";
        const count = Math.max(labels.length, ...series.map((s) => s.values.length), 0);
        if (count === 0) return null;
        const scale = Scale.nice(series.flatMap((s) => s.values), kind !== "line");
        const ticks = scale.ticks();
        const tickLabels = ticks.map((t) => scale.label(t));
        const longest = Math.max(1, ...tickLabels.map((l) => l.length));
        const left = Math.ceil(longest * 6.1) + 10;
        const top = bars ? 16 : 8;
        const right = width - (bars ? 0 : 6);
        const cols = Math.max(0, Math.floor((right - left) / CELL));
        const rows = Math.max(0, Math.floor((height - top - 22) / CELL));
        const plotW = cols * CELL;
        const plotH = rows * CELL;
        const yOf = (v: number) => scale.depth(v) * plotH;
        const band = plotW / count;
        const xOf = (i: number) =>
          bars ? band * (i + 0.5) : count <= 1 ? plotW / 2 : (i * plotW) / (count - 1);
        const tints = series.map((s) => color(s.color ?? "series1"));
        const stride = Math.max(1, Math.ceil(count / Math.max(1, Math.floor(plotW / 72))));

        let marks: ReactNode = null;
        let lastValue: ReactNode = null;
        if (bars) {
          const n = Math.max(1, series.length);
          const widthPx = Math.min((band * 0.7 - CELL * (n - 1)) / n, MAX_BAR);
          const wide = Math.max(1, Math.round(widthPx / CELL));
          const group = wide * n + (n - 1);
          const rowOf = (v: number) => Math.round((scale.depth(v) * plotH) / CELL);
          const zero = rowOf(Math.min(Math.max(scale.min, 0), scale.max));
          const fade = Math.max(1, rows * BAR_FADE);
          const columns: ([number, number, number] | null)[] = new Array(cols).fill(null);
          let lastTop: number | null = null;
          for (let i = 0; i < count; i++) {
            const start = Math.round((band * (i + 0.5)) / CELL - group / 2);
            series.forEach((s, j) => {
              const value = s.values[i];
              if (value === undefined) return;
              const end = rowOf(value);
              const first = start + j * (wide + 1);
              for (let col = first; col < first + wide; col++)
                if (col >= 0 && col < cols) columns[col] = [Math.min(end, zero), Math.max(end, zero), j];
              if (i === count - 1) lastTop = Math.min(end, zero);
            });
          }
          marks = (
            <Cells
              x={left}
              y={top}
              paths={cells(cols, rows, (col, row) => {
                const bar = columns[col];
                if (!bar || row < bar[0] || row >= bar[1]) return null;
                const tone = BAR_LEVEL - (BAR_LEVEL - BAR_FLOOR) * Math.min((row - bar[0]) / fade, 1);
                return row === bar[0] || inked(col, row, tone) ? tints[bar[2]] : null;
              })}
            />
          );
          // With one series, the latest value is written over its bar.
          const latest = series[0]?.values[count - 1];
          if (series.length === 1 && lastTop !== null && latest !== undefined)
            lastValue = (
              <SvgText
                {...axis}
                fill={withAlpha(colors.content, 0.9)}
                x={left + band * (count - 0.5)}
                y={top + lastTop * CELL - 5}
                textAnchor="middle"
              >
                {compact(latest, scale.step / 10)}
              </SvgText>
            );
        } else {
          const curves = series.map(
            (s) => new Curve(s.values.map((v, i) => [xOf(i), yOf(v)] as [number, number])),
          );
          const area =
            kind === "area"
              ? (() => {
                  const baseline = yOf(Math.max(scale.min, 0));
                  return (
                    <>
                      {curves.map((curve, index) => (
                        <Path
                          key={`wash${index}`}
                          x={left}
                          y={top}
                          d={`${curve.path(4)}L${curve.lastX} ${baseline}L${curve.firstX} ${baseline}z`}
                          fill={tints[index]}
                          fillOpacity={dark ? 0.07 : 0.04}
                        />
                      ))}
                      <Cells
                        x={left}
                        y={top}
                        paths={cells(cols, rows, (col, row) => {
                          const [x, y] = center(col, row);
                          if (y > baseline) return null;
                          // Later series sit in front, as stacked paper would.
                          for (let index = curves.length - 1; index >= 0; index--) {
                            const curve = curves[index];
                            if (x < curve.firstX || x > curve.lastX || y < curve.at(x)) continue;
                            const depth = (y - curve.at(x)) / Math.max(plotH, 1);
                            const lead = Math.max((2 * CELL) / Math.max(plotH, 1), AREA_BANDS[0][0]);
                            const share =
                              depth < lead ? 1 : (AREA_BANDS.find(([d]) => depth < d)?.[1] ?? AREA_REST);
                            return inked(col, row, step(AREA_LEVEL * share)) ? tints[index] : null;
                          }
                          return null;
                        })}
                      />
                    </>
                  );
                })()
              : null;
          marks = (
            <>
              {area}
              {curves.map((curve, index) => (
                <Line key={`line${index}`} d={curve.path(2)} color={tints[index]} x={left} y={top} width={2} />
              ))}
              {series.map((s, index) => {
                const last = s.values.length - 1;
                if (last < 0) return null;
                return <Marker key={`end${index}`} x={left + xOf(last)} y={top + yOf(s.values[last])} color={tints[index]} />;
              })}
            </>
          );
        }
        return (
          <Svg width={width} height={height}>
            {ticks.map((tick, index) => {
              const y = top + Math.round(yOf(tick));
              return (
                <G key={tick}>
                  <Rect x={left} y={y} width={plotW} height={1} fill={withAlpha(colors.content, 0.07)} />
                  <SvgText {...axis} x={left - 10} y={y + 3.5} textAnchor="end">
                    {tickLabels[index]}
                  </SvgText>
                </G>
              );
            })}
            {labels.map((label, i) =>
              i % stride ? null : (
                <SvgText
                  key={i}
                  {...axis}
                  x={left + xOf(i)}
                  y={top + plotH + 16}
                  textAnchor={bars ? "middle" : i === 0 ? "start" : i + 1 === count ? "end" : "middle"}
                >
                  {label.length > 12 ? `${label.slice(0, 11)}…` : label}
                </SvgText>
              ),
            )}
            {marks}
            {lastValue}
          </Svg>
        );
      }}
    </Measured>
  );
}

/** A line with a soft glow under it, as `glowing_line`. */
function Line({ d, color, x, y, width }: { d: string; color: string; x: number; y: number; width: number }) {
  const dark = useScheme() === "dark";
  const dim = dark ? 1 : 0.45;
  return (
    <>
      <Path d={d} x={x} y={y} stroke={color} strokeOpacity={0.07 * dim} strokeWidth={width + 10} fill="none" strokeLinecap="round" strokeLinejoin="round" />
      <Path d={d} x={x} y={y} stroke={color} strokeOpacity={0.14 * dim} strokeWidth={width + 5} fill="none" strokeLinecap="round" strokeLinejoin="round" />
      <Path d={d} x={x} y={y} stroke={color} strokeWidth={width} fill="none" strokeLinecap="round" strokeLinejoin="round" />
    </>
  );
}

/** A square end marker in a faint halo of its colour. */
function Marker({ x, y, color }: { x: number; y: number; color: string }) {
  const side = 2 * CELL;
  return (
    <>
      <Rect x={x - (side + 6) / 2} y={y - (side + 6) / 2} width={side + 6} height={side + 6} fill={color} opacity={0.25} />
      <Rect x={x - side / 2} y={y - side / 2} width={side} height={side} fill={color} />
    </>
  );
}

/** The sparkline under a stat: a line over bands that step lighter downward. */
export function Trend({ values, tint }: { values: number[]; tint: string }) {
  const HEIGHT = 56;
  const LEVELS = [10 / 16, 6 / 16, 3 / 16, 1 / 16];
  return (
    <Measured height={HEIGHT}>
      {(width) => {
        if (values.length < 2) return null;
        const cols = Math.floor(width / CELL);
        const rows = Math.floor(HEIGHT / CELL);
        const w = cols * CELL;
        const h = rows * CELL;
        const low = Math.min(...values);
        const high = Math.max(...values);
        const range = high > low ? high - low : 1;
        const top = 6;
        const floor = h * 0.3;
        const curve = new Curve(
          values.map((v, i) => [(i / (values.length - 1)) * w, top + (1 - (v - low) / range) * (h - top - floor)]),
        );
        return (
          <Svg width={width} height={HEIGHT}>
            <Cells
              paths={cells(cols, rows, (col, row) => {
                const [x, y] = center(col, row);
                const depth = y - curve.at(x);
                if (depth < 0) return null;
                return inked(col, row, LEVELS[Math.min(Math.floor(depth / 7), LEVELS.length - 1)]) ? tint : null;
              })}
            />
            <Line d={curve.path(2)} color={tint} x={0} y={0} width={1.5} />
            <Marker x={w - CELL} y={curve.at(w)} color={tint} />
          </Svg>
        );
      }}
    </Measured>
  );
}

/** A radial dial of dithered blocks. */
export function Gauge({
  label,
  value,
  min,
  max,
  unit,
  tint,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  unit?: string | null;
  tint: string;
}) {
  const colors = useColors();
  const axis = useAxis();
  const SEGMENTS = 36;
  const SPAN = 270;
  const START = -135;
  const size = 220;
  const ratio = Math.min(Math.max((value - min) / (max - min || 1), 0), 1);
  const format = (v: number) =>
    unit === "%" ? `${compact(v, 1)}%` : unit ? `${compact(v, 0.1)} ${unit}` : compact(v, 0.1);
  const outerFull = size / 2 - 1 - CELL;
  const height = size / 2 + (outerFull + CELL) * Math.SQRT1_2 + 18;
  return (
    <Measured height={height}>
      {(width) => {
        const side = Math.min(size, width);
        const offset = Math.round((width - side) / 2);
        const c = side / 2;
        const outer = side / 2 - 1 - CELL;
        const thickness = Math.max(2, Math.round((side * 0.12) / CELL)) * CELL;
        const inner = outer - thickness;
        const segment = SPAN / SEGMENTS;
        const middle = outer - thickness / 2;
        const gap = ((2 / middle) * 180) / Math.PI;
        const filled = ratio * SEGMENTS;
        const active = Math.max(0, Math.ceil(filled - 1e-6));
        const track = withAlpha(tint, 0.14);
        const paths = cells(Math.floor(side / CELL), Math.floor(height / CELL), (col, row) => {
          const [x, y] = center(col, row);
          const dx = x - c;
          const dy = y - c;
          const r = Math.hypot(dx, dy);
          if (r < inner || r > outer + CELL) return null;
          const along = (Math.atan2(dx, -dy) * 180) / Math.PI - START;
          if (along < 0 || along >= SPAN) return null;
          const index = Math.floor(along / segment);
          const within = along - index * segment;
          if (within < gap / 2 || within > segment - gap / 2) return null;
          const head = active > 0 && index === active - 1;
          if (r > outer && !head) return null;
          if (index >= active) return track;
          if (head) {
            const part = Math.min(Math.max(filled - index, 0), 1);
            if (within > gap / 2 + (segment - gap) * Math.max(part, 0.15)) return r <= outer ? track : null;
            return tint;
          }
          const tone = Math.max(1 - (active - 1 - index) / 16, 9 / 16);
          return inked(col, row, tone) ? tint : null;
        });
        const ends = middle * Math.SQRT1_2;
        return (
          <Svg width={width} height={height}>
            <Cells paths={paths} x={offset} />
            <SvgText
              x={offset + c}
              y={c - 4 + side * 0.05}
              fontSize={Math.round(side * 0.15)}
              fontWeight="600"
              fill={colors.content}
              textAnchor="middle"
            >
              {format(value)}
            </SvgText>
            <SvgText x={offset + c} y={c + side * 0.1 + 10} fontSize={12} fill={withAlpha(colors.content, 0.6)} textAnchor="middle">
              {label}
            </SvgText>
            {([[-ends, min], [ends, max]] as const).map(([dx, v]) => (
              <SvgText key={dx} {...axis} x={offset + c + dx} y={c + ends + thickness / 2 + 15} textAnchor="middle">
                {compact(v, 1)}
              </SvgText>
            ))}
          </Svg>
        );
      }}
    </Measured>
  );
}

/** Parts of a whole as a dithered ring with a legend. */
export function Donut({ slices, label, value }: { slices: Slice[]; label?: string | null; value?: string | null }) {
  const color = useVisualColor();
  const colors = useColors();
  const axis = useAxis();
  const SIZE = 184;
  const FLOOR = 3 / 16;
  const total = slices.reduce((sum, s) => sum + Math.max(s.value, 0), 0);
  const figure = value ?? (Math.abs(total) >= 1e4 ? Math.round(total).toLocaleString() : compact(total, 0.1));
  const tints = slices.map((s) => color(s.color ?? "series1"));
  const grid = Math.floor(SIZE / CELL);
  const half = (grid * CELL) / 2;
  const outer = half - 2 * CELL;
  const inner = outer - Math.round((SIZE * 0.17) / CELL) * CELL;
  const TAU = Math.PI * 2;
  const angleOf = (dx: number, dy: number) => {
    const a = Math.atan2(dx, -dy);
    return a < 0 ? a + TAU : a;
  };
  const edges: number[] = [];
  let running = 0;
  for (const s of slices) {
    edges.push((running / Math.max(total, 1e-9)) * TAU);
    running += Math.max(s.value, 0);
  }
  const shown = slices.filter((s) => s.value > 0).length;
  const paths = cells(grid, grid, (col, row) => {
    const [x, y] = center(col, row);
    const dx = x - half;
    const dy = y - half;
    const r = Math.hypot(dx, dy);
    if (r < inner || r > outer) return null;
    const angle = angleOf(dx, dy);
    let index = -1;
    for (let i = edges.length - 1; i >= 0; i--)
      if (angle >= edges[i]) {
        index = i;
        break;
      }
    if (index < 0) return null;
    let outline = r > outer - CELL || r < inner + CELL;
    if (shown > 1) {
      let near = Infinity;
      edges.forEach((edge, i) => {
        if (slices[i].value <= 0) return;
        let d = Math.abs(angle - edge);
        d = Math.min(d, TAU - d);
        if (d <= Math.PI / 2) near = Math.min(near, (r * Math.sin(d)) / CELL);
      });
      if (near < 0.75) return null;
      outline ||= near < 1.75;
    }
    const depth = (outer - r) / (outer - inner);
    const tone = FLOOR + (1 - FLOOR) * (1 - depth) ** 1.4;
    return outline || inked(col, row, tone) ? tints[index] : null;
  });
  return (
    <View style={styles.donut}>
      <Svg width={SIZE} height={SIZE}>
        <Cells paths={paths} />
        {label ? (
          <SvgText {...axis} x={half} y={half - 12} textAnchor="middle">
            {label.toUpperCase().slice(0, 18)}
          </SvgText>
        ) : null}
        <SvgText
          x={half}
          y={half + (label ? 12 : 8)}
          fontSize={Math.floor(Math.min(26, (inner * 2 - 16) / (Math.max(figure.length, 1) * 0.62)))}
          fontWeight="600"
          fill={colors.content}
          textAnchor="middle"
        >
          {figure}
        </SvgText>
      </Svg>
      <View style={styles.legend}>
        {slices.map((s, index) => (
          <View key={`${s.label}${index}`} style={styles.legendRow}>
            <View style={[styles.swatch, { backgroundColor: tints[index] }]} />
            <Text style={[styles.legendLabel, { color: withAlpha(colors.content, 0.8) }]} numberOfLines={1}>
              {s.label}
            </Text>
            <Text style={[styles.legendValue, { color: withAlpha(colors.content, 0.5) }]}>
              {compact(s.value, 0.1)} · {(total > 0 ? (s.value / total) * 100 : 0).toFixed(0)}%
            </Text>
          </View>
        ))}
      </View>
    </View>
  );
}

/** Values by row and column as dithered squares, as the desktop's heatmap. */
export function Heatmap({
  values,
  rows,
  columns,
  tint,
}: {
  values: number[][];
  rows: string[];
  columns: string[];
  tint: string;
}) {
  const colors = useColors();
  const axis = useAxis();
  const SQUARE = 4;
  const PITCH = 5;
  const width = values[0]?.length ?? 0;
  const gutter = rows.length ? 34 : 0;
  const top = columns.length ? 18 : 0;
  const height = top + values.length * PITCH * CELL;
  const high = Math.max(1e-9, ...values.flat());
  const track = withAlpha(colors.content, 0.07);
  return (
    <Measured height={height}>
      {(available) => {
        const cols = Math.floor((available - gutter) / CELL);
        const fits = Math.max(1, Math.floor((cols + PITCH - SQUARE) / PITCH));
        const skip = Math.max(0, width - fits);
        const pitch = PITCH * CELL;
        const paths = cells(cols, values.length * PITCH, (col, row) => {
          const x = col % PITCH;
          const y = row % PITCH;
          if (x >= SQUARE || y >= SQUARE || Math.floor(col / PITCH) >= fits) return null;
          const v = values[Math.floor(row / PITCH)]?.[Math.floor(col / PITCH) + skip];
          if (v === undefined) return null;
          if (v <= 0) return track;
          return inked(x, y, step(0.25 + 0.75 * (v / high))) ? tint : track;
        });
        let previous: [string, number] | null = null;
        return (
          <Svg width={available} height={height}>
            <Cells paths={paths} x={gutter} y={top} />
            {rows.map((label, i) =>
              rows.length > 4 && i % 2 === 1 ? null : (
                <SvgText key={i} {...axis} x={0} y={top + i * pitch + (SQUARE * CELL) / 2 + 3.5}>
                  {label}
                </SvgText>
              ),
            )}
            {columns.slice(skip).map((label, i) => {
              const x = i * pitch;
              if (previous && (previous[0] === label || x - previous[1] < 36)) return null;
              previous = [label, x];
              return (
                <SvgText key={i} {...axis} x={gutter + x} y={top / 2 + 3.5}>
                  {label}
                </SvgText>
              );
            })}
          </Svg>
        );
      }}
    </Measured>
  );
}

/** A labelled bar of dithered cells, denser toward its head. */
export function Progress({
  label,
  value,
  max,
  detail,
  tint,
}: {
  label: string;
  value: number;
  max: number;
  detail?: string | null;
  tint: string;
}) {
  const colors = useColors();
  const ratio = Math.min(Math.max(value / (max || 1), 0), 1);
  return (
    <View style={styles.progress}>
      <View style={styles.progressHead}>
        <Text style={[styles.progressLabel, { color: colors.content }]} numberOfLines={1}>
          {label}
        </Text>
        <Text style={[styles.mono, { color: withAlpha(colors.content, 0.55) }]}>{(ratio * 100).toFixed(0)}%</Text>
      </View>
      <Measured height={3 * CELL}>
        {(width) => {
          const cols = Math.floor(width / CELL);
          const head = Math.round(cols * ratio);
          const track = withAlpha(tint, 0.14);
          return (
            <Svg width={width} height={3 * CELL}>
              <Cells
                paths={cells(cols, 3, (col, row) => {
                  if (col >= head) return track;
                  const tone = 0.3 + 0.7 * (col / Math.max(head, 1)) ** 1.5;
                  return col >= head - 3 || inked(col, row, tone) ? tint : track;
                })}
              />
            </Svg>
          );
        }}
      </Measured>
      {detail ? <Text style={[styles.detail, { color: withAlpha(colors.content, 0.55) }]}>{detail}</Text> : null}
    </View>
  );
}

/**
 * A Mono's contributions as the desktop's dashboard draws them: weeks as
 * columns, Sunday to Saturday down, each day a square of 4×4 dither cells in
 * the accent, denser the busier it was. Day by day from a Sunday.
 */
export function ContributionGraph({ levels }: { levels: number[] }) {
  const colors = useColors();
  const axis = useAxis();
  const TONES = [0, 0.25, 0.5, 0.75, 1];
  const SQUARE = 4;
  const PITCH = 5;
  const GUTTER = 28;
  const MONTHS = 18;
  const height = MONTHS + 7 * PITCH * CELL;
  const weeks = Math.ceil(levels.length / 7);
  // The first day shown: today, back as many days as the graph has.
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  start.setDate(start.getDate() - (levels.length - 1));
  return (
    <Measured height={height}>
      {(width) => {
        const cols = Math.floor((width - GUTTER) / CELL);
        // A narrow screen keeps the latest weeks, as calendars do.
        const fits = Math.min(Math.max(1, Math.floor((cols + PITCH - SQUARE) / PITCH)), weeks);
        const skip = weeks - fits;
        const track = withAlpha(colors.content, 0.07);
        const pitch = PITCH * CELL;
        const paths = cells(fits * PITCH, 7 * PITCH, (col, row) => {
          const x = col % PITCH;
          const y = row % PITCH;
          const c = Math.floor(col / PITCH);
          const r = Math.floor(row / PITCH);
          if (x >= SQUARE || y >= SQUARE) return null;
          const level = levels[(c + skip) * 7 + r];
          if (level === undefined) return null;
          if (level === 0) return track;
          return inked(x, y, TONES[level]) ? colors.accent : track;
        });
        const months: ReactNode[] = [];
        let lastAt = -Infinity;
        for (let c = 0; c < fits; c++) {
          const week = new Date(start);
          week.setDate(start.getDate() + (c + skip) * 7);
          const x = c * pitch;
          if ((c === 0 || week.getDate() <= 7) && x - lastAt >= 30) {
            lastAt = x;
            months.push(
              <SvgText key={c} {...axis} x={GUTTER + x} y={MONTHS / 2 + 3.5}>
                {week.toLocaleDateString([], { month: "short" })}
              </SvgText>,
            );
          }
        }
        return (
          <Svg width={width} height={height}>
            <Cells paths={paths} x={GUTTER} y={MONTHS} />
            {[
              [1, "Mon"],
              [3, "Wed"],
              [5, "Fri"],
            ].map(([r, name]) => (
              <SvgText key={name} {...axis} x={0} y={MONTHS + (r as number) * pitch + (SQUARE * CELL) / 2 + 3.5}>
                {name}
              </SvgText>
            ))}
            {months}
          </Svg>
        );
      }}
    </Measured>
  );
}

const styles = StyleSheet.create({
  donut: {
    flexDirection: "row",
    flexWrap: "wrap",
    alignItems: "center",
    gap: 24,
  },
  legend: {
    flex: 1,
    minWidth: 140,
    gap: 8,
  },
  legendRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  swatch: {
    width: 9,
    height: 9,
  },
  legendLabel: {
    flex: 1,
    fontSize: 12,
  },
  legendValue: {
    fontSize: 12,
    fontFamily: "Menlo",
  },
  progress: {
    gap: 8,
  },
  progressHead: {
    flexDirection: "row",
    alignItems: "baseline",
    gap: 8,
  },
  progressLabel: {
    flex: 1,
    fontSize: 13,
  },
  mono: {
    fontSize: 12,
    fontFamily: "Menlo",
  },
  detail: {
    fontSize: 12,
  },
});

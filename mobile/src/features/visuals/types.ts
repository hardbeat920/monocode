// A visual's component tree as the desktop renders it
// (crates/monocode-core/src/visual/model.rs): data, never HTML.

export type ThemeColor =
  | "ink" | "muted" | "faint" | "accent" | "positive" | "negative" | "warning"
  | "series1" | "series2" | "series3" | "series4" | "series5" | "series6";
export type TextSize = "caption" | "small" | "body" | "subtitle" | "title" | "hero";
export type Series = { label: string; values: number[]; color?: ThemeColor };
export type Slice = { label: string; value: number; color?: ThemeColor };

export type Shape =
  | { kind: "rect"; x: number; y: number; width: number; height: number; color: ThemeColor }
  | { kind: "circle"; x: number; y: number; radius: number; color: ThemeColor }
  | { kind: "line"; x1: number; y1: number; x2: number; y2: number; width: number; color: ThemeColor }
  | { kind: "polyline"; points: [number, number][]; width: number; color: ThemeColor }
  | { kind: "label"; x: number; y: number; text: string; color: ThemeColor };

export type VisualNode =
  | { kind: "column" | "row" | "card"; children: VisualNode[]; gap: number }
  | { kind: "grid"; children: VisualNode[]; columns: number; gap: number }
  | { kind: "text"; text: string; size: TextSize; color: ThemeColor }
  | {
      kind: "stat";
      label: string;
      value: string;
      detail?: string | null;
      color: ThemeColor;
      delta?: string | null;
      trend?: number[];
      lower_is_better?: boolean;
    }
  | { kind: "table"; columns: string[]; rows: string[][] }
  | { kind: "chart"; chart_type: "bar" | "line" | "area"; series: Series[]; labels: string[]; height: number }
  | { kind: "gauge"; label: string; value: number; min: number; max: number; unit?: string | null; color: ThemeColor }
  | { kind: "donut"; slices: Slice[]; label?: string | null; value?: string | null }
  | { kind: "heatmap"; values: number[][]; rows: string[]; columns: string[]; color: ThemeColor }
  | { kind: "progress"; label: string; value: number; max: number; detail?: string | null; color: ThemeColor }
  | { kind: "button"; label: string; handler: string; disabled: boolean }
  | { kind: "toggle"; label: string; value: boolean; handler: string }
  | { kind: "input"; label: string; value: string; placeholder?: string | null; handler: string }
  | { kind: "select"; label: string; options: string[]; selected: number; handler: string }
  | { kind: "slider"; label: string; value: number; min: number; max: number; step: number; handler: string }
  | { kind: "tabs"; labels: string[]; selected: number; handler: string }
  | { kind: "canvas"; width: number; height: number; shapes: Shape[] }
  | { kind: "scene"; width: number; height: number };

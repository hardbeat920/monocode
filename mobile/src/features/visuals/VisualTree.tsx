// A visual's component tree drawn natively, as the desktop's VisualView lays
// it out (crates/monocode-gpui/src/visuals.rs). Its controls show their state
// but don't act yet: the visual runs on the desktop.
import { useState } from "react";
import { ScrollView, StyleSheet, Text, View } from "react-native";
import Svg, { Circle, Line, Polyline, Rect, Text as SvgText } from "react-native-svg";

import { radii, themedStyles, useColors, withAlpha } from "@/ui/theme";

import { Cartesian, Donut, Gauge, Heatmap, Progress, Trend, useVisualColor } from "./charts";
import type { TextSize, VisualNode } from "./types";

const TEXT_SIZES: Record<TextSize, number> = {
  caption: 11,
  small: 12,
  body: 14,
  subtitle: 17,
  title: 22,
  hero: 34,
};

export function VisualTree({ node }: { node: VisualNode }) {
  return <Node node={node} />;
}

function Node({ node }: { node: VisualNode }) {
  const styles = useStyles();
  const colors = useColors();
  const color = useVisualColor();
  const [width, setWidth] = useState(0);
  switch (node.kind) {
    case "column":
    case "card":
      return (
        <View style={[{ gap: node.gap }, node.kind === "card" && styles.card]}>
          {node.children.map((child, i) => (
            <Node key={i} node={child} />
          ))}
        </View>
      );
    case "row":
      return (
        <View style={[styles.row, { gap: node.gap }]}>
          {node.children.map((child, i) => (
            <View key={i} style={styles.rowItem}>
              <Node node={child} />
            </View>
          ))}
        </View>
      );
    case "grid": {
      // As many columns as fit at 180pt each, at most the visual's own.
      const columns = Math.max(1, Math.min(node.columns, Math.floor(width / 180) || 1));
      const cell = width > 0 ? (width - node.gap * (columns - 1)) / columns : 0;
      return (
        <View style={[styles.row, { gap: node.gap }]} onLayout={(e) => setWidth(e.nativeEvent.layout.width)}>
          {width > 0
            ? node.children.map((child, i) => (
                <View key={i} style={{ width: cell }}>
                  <Node node={child} />
                </View>
              ))
            : null}
        </View>
      );
    }
    case "text": {
      const size = TEXT_SIZES[node.size] ?? 14;
      return (
        <Text
          style={{
            color: color(node.color ?? "ink"),
            fontSize: size,
            lineHeight: Math.round(size * 1.45),
            fontWeight: ["subtitle", "title", "hero"].includes(node.size) ? "600" : "400",
          }}
        >
          {node.text}
        </Text>
      );
    }
    case "stat": {
      let delta = node.delta ?? null;
      let detail = node.detail ?? null;
      // "+12.4% vs last month" lends its leading change to the delta.
      if (!delta && detail) {
        const match = /^([+\-−]\d\S*)\s*(.*)$/.exec(detail);
        if (match) {
          delta = match[1];
          detail = match[2] || null;
        }
      }
      const tint = ["ink", "muted", "faint"].includes(node.color) ? colors.accent : color(node.color);
      const down = !!delta && (delta.startsWith("-") || delta.startsWith("−"));
      return (
        <View style={styles.stat}>
          <Text style={styles.statLabel} numberOfLines={1}>{node.label}</Text>
          <Text style={[styles.statValue, { color: color(node.color ?? "ink") }]}>{node.value}</Text>
          {delta || detail ? (
            <View style={styles.statLine}>
              {delta ? (
                <Text style={[styles.mono, { color: color(down !== !!node.lower_is_better ? "negative" : "positive") }]}>
                  {down ? "▼" : "▲"} {delta}
                </Text>
              ) : null}
              {detail ? <Text style={styles.small}>{detail}</Text> : null}
            </View>
          ) : null}
          {node.trend && node.trend.length > 1 ? (
            <View style={styles.trend}>
              <Trend values={node.trend} tint={tint} />
            </View>
          ) : null}
        </View>
      );
    }
    case "table":
      return (
        <ScrollView horizontal showsHorizontalScrollIndicator={false}>
          <View>
            {[node.columns, ...node.rows].map((cells, r) => (
              <View key={r} style={styles.tableRow}>
                {cells.map((cell, c) => (
                  <Text key={c} style={[styles.tableCell, r === 0 && styles.tableHead]}>
                    {cell}
                  </Text>
                ))}
              </View>
            ))}
          </View>
        </ScrollView>
      );
    case "chart":
      return <Cartesian kind={node.chart_type} series={node.series} labels={node.labels} height={node.height || 220} />;
    case "gauge":
      return <Gauge {...node} tint={color(node.color)} />;
    case "donut":
      return <Donut slices={node.slices} label={node.label} value={node.value} />;
    case "heatmap":
      return <Heatmap values={node.values} rows={node.rows} columns={node.columns} tint={color(node.color)} />;
    case "progress":
      return <Progress {...node} tint={color(node.color)} />;
    case "button":
      return (
        <View style={[styles.control, node.disabled && styles.off]}>
          <Text style={styles.controlLabel}>{node.label}</Text>
        </View>
      );
    case "toggle":
      return (
        <View style={styles.field}>
          <Text style={styles.fieldLabel}>{node.label}</Text>
          <View style={[styles.switch, node.value && { backgroundColor: colors.accent }]}>
            <View style={[styles.knob, node.value && styles.knobOn]} />
          </View>
        </View>
      );
    case "input":
      return (
        <View style={styles.stack}>
          <Text style={styles.small}>{node.label}</Text>
          <View style={styles.box}>
            <Text style={node.value ? styles.body : styles.small}>{node.value || node.placeholder || ""}</Text>
          </View>
        </View>
      );
    case "select":
      return (
        <View style={styles.field}>
          <Text style={styles.fieldLabel}>{node.label}</Text>
          <Text style={styles.small}>{node.options[node.selected] ?? ""}</Text>
        </View>
      );
    case "slider": {
      const share = Math.min(Math.max((node.value - node.min) / (node.max - node.min || 1), 0), 1);
      return (
        <View style={styles.stack}>
          <View style={styles.field}>
            <Text style={styles.fieldLabel}>{node.label}</Text>
            <Text style={styles.mono}>{node.value}</Text>
          </View>
          <View style={styles.track}>
            <View style={[styles.fill, { width: `${share * 100}%`, backgroundColor: colors.accent }]} />
          </View>
        </View>
      );
    }
    case "tabs":
      return (
        <View style={styles.tabs}>
          {node.labels.map((label, i) => (
            <View key={label} style={[styles.tab, i === node.selected && styles.tabOn]}>
              <Text style={[styles.small, i === node.selected && { color: colors.content }]}>{label}</Text>
            </View>
          ))}
        </View>
      );
    case "canvas":
      return (
        <Svg width="100%" height={undefined} viewBox={`0 0 ${node.width} ${node.height}`} style={{ aspectRatio: node.width / node.height }}>
          {node.shapes.map((shape, i) => {
            const tint = color(shape.color);
            switch (shape.kind) {
              case "rect":
                return <Rect key={i} x={shape.x} y={shape.y} width={shape.width} height={shape.height} fill={tint} />;
              case "circle":
                return <Circle key={i} cx={shape.x} cy={shape.y} r={shape.radius} fill={tint} />;
              case "line":
                return <Line key={i} x1={shape.x1} y1={shape.y1} x2={shape.x2} y2={shape.y2} stroke={tint} strokeWidth={shape.width} />;
              case "polyline":
                return (
                  <Polyline key={i} points={shape.points.map((p) => p.join(",")).join(" ")} stroke={tint} strokeWidth={shape.width} fill="none" />
                );
              case "label":
                return <SvgText key={i} x={shape.x} y={shape.y} fill={tint} fontSize={12}>{shape.text}</SvgText>;
            }
          })}
        </Svg>
      );
    case "scene":
      return (
        <View style={[styles.card, styles.scene, { aspectRatio: node.width / node.height || 16 / 9 }]}>
          <Text style={styles.small}>Open this 3D scene on your desktop to see it.</Text>
        </View>
      );
    default:
      return null;
  }
}

const useStyles = themedStyles((colors) => ({
  card: {
    padding: 16,
    borderRadius: 16,
    borderCurve: "continuous",
    backgroundColor: withAlpha(colors.content, 0.03),
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: withAlpha(colors.content, 0.08),
  },
  row: {
    flexDirection: "row",
    flexWrap: "wrap",
    alignItems: "flex-start",
  },
  rowItem: {
    flex: 1,
    minWidth: 120,
  },
  stat: {
    gap: 4,
  },
  statLabel: {
    color: withAlpha(colors.content, 0.6),
    fontSize: 13,
  },
  statValue: {
    fontSize: 30,
    lineHeight: 38,
    fontWeight: "600",
    fontVariant: ["tabular-nums"],
  },
  statLine: {
    flexDirection: "row",
    flexWrap: "wrap",
    alignItems: "center",
    columnGap: 8,
  },
  trend: {
    paddingTop: 8,
  },
  mono: {
    color: withAlpha(colors.content, 0.55),
    fontSize: 12,
    fontFamily: "Menlo",
  },
  small: {
    color: withAlpha(colors.content, 0.55),
    fontSize: 12,
  },
  body: {
    color: colors.content,
    fontSize: 13,
  },
  tableRow: {
    flexDirection: "row",
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: withAlpha(colors.content, 0.08),
  },
  tableCell: {
    width: 110,
    paddingHorizontal: 8,
    paddingVertical: 8,
    color: colors.content,
    fontSize: 12,
  },
  tableHead: {
    fontWeight: "600",
    color: withAlpha(colors.content, 0.65),
  },
  control: {
    alignSelf: "flex-start",
    height: 30,
    paddingHorizontal: 12,
    justifyContent: "center",
    borderRadius: 8,
    backgroundColor: withAlpha(colors.content, 0.08),
  },
  controlLabel: {
    color: colors.content,
    fontSize: 13,
  },
  off: {
    opacity: 0.4,
  },
  field: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: 12,
  },
  fieldLabel: {
    flex: 1,
    color: colors.content,
    fontSize: 13,
  },
  stack: {
    gap: 6,
  },
  box: {
    paddingHorizontal: 10,
    paddingVertical: 8,
    borderRadius: 8,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: withAlpha(colors.content, 0.15),
  },
  switch: {
    width: 34,
    height: 20,
    borderRadius: 10,
    padding: 2,
    backgroundColor: withAlpha(colors.content, 0.15),
  },
  knob: {
    width: 16,
    height: 16,
    borderRadius: 8,
    backgroundColor: "#ffffff",
  },
  knobOn: {
    marginLeft: 14,
  },
  track: {
    height: 4,
    borderRadius: 2,
    backgroundColor: withAlpha(colors.content, 0.12),
    overflow: "hidden",
  },
  fill: {
    height: 4,
  },
  tabs: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: 4,
    padding: 3,
    alignSelf: "flex-start",
    borderRadius: radii.row,
    backgroundColor: withAlpha(colors.content, 0.06),
  },
  tab: {
    paddingHorizontal: 10,
    paddingVertical: 5,
    borderRadius: radii.row - 3,
  },
  tabOn: {
    backgroundColor: withAlpha(colors.content, 0.1),
  },
  scene: {
    alignItems: "center",
    justifyContent: "center",
  },
}));

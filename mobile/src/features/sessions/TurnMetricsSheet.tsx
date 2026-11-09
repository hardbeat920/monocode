import { BottomSheet, RNHostView } from "@expo/ui";
import { Divider, HStack, Spacer, Text, VStack } from "@expo/ui/swift-ui";
import {
  background,
  font,
  foregroundStyle,
  monospacedDigit,
  padding,
  shapes,
} from "@expo/ui/swift-ui/modifiers";
import { Fragment } from "react";
import { Platform, StyleSheet, Text as RNText, View, useWindowDimensions } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import type { TurnMetrics } from "@/features/workspace/types";
import { themedStyles, withAlpha, useColors } from "@/ui/theme";

import {
  formatClockTime,
  turnMetricsSheet,
  type MetricStat,
} from "./transcriptFormat";

const CARD_SHAPE = shapes.roundedRectangle({
  cornerRadius: 20,
  roundedCornerStyle: "continuous",
});
const SECONDARY = foregroundStyle({ type: "hierarchical", style: "secondary" });

/**
 * Native bottom sheet with one turn's token metrics, sized to fit them: its
 * rates, then each token count in full, as rows. Built from
 * SwiftUI views rather than React Native ones, so the sheet can measure the
 * content and rise only as far as it needs.
 */
export function TurnMetricsSheet({
  isPresented,
  onDismiss,
  metrics,
  elapsedMs,
  completedAt,
}: {
  isPresented: boolean;
  onDismiss: () => void;
  metrics: TurnMetrics;
  elapsedMs: number | null;
  completedAt?: number;
}) {
  const { highlights, tokens } = turnMetricsSheet(metrics, elapsedMs);
  // SwiftUI stacks have no Android view, so Android builds the same sheet
  // from React Native views.
  if (Platform.OS === "android") return (
    <AndroidSheet
      isPresented={isPresented}
      onDismiss={onDismiss}
      highlights={highlights}
      tokens={tokens}
      completedAt={completedAt}
    />
  );
  return (
    <BottomSheet
      isPresented={isPresented}
      onDismiss={onDismiss}
      contentPadding={{ top: 28, left: 20, right: 20, bottom: 12 }}
    >
      <VStack alignment="leading" spacing={20}>
        <VStack alignment="leading" spacing={2}>
          <Text modifiers={[font({ textStyle: "title3", weight: "semibold" })]}>
            Turn metrics
          </Text>
          {completedAt != null ? (
            <Text modifiers={[font({ textStyle: "subheadline" }), SECONDARY]}>
              {`Finished at ${formatClockTime(completedAt)}`}
            </Text>
          ) : null}
        </VStack>

        {highlights.length > 0 ? <StatGroup stats={highlights} /> : null}
        {tokens.length > 0 ? <StatGroup title="Tokens" stats={tokens} /> : null}
      </VStack>
    </BottomSheet>
  );
}

/** Stats as rows in one rounded card, under an optional heading. */
function StatGroup({ title, stats }: { title?: string; stats: MetricStat[] }) {
  const colors = useColors();
  return (
    <VStack alignment="leading" spacing={8}>
      {title ? (
        <Text
          modifiers={[
            font({ textStyle: "footnote", weight: "semibold" }),
            SECONDARY,
            padding({ leading: 4 }),
          ]}
        >
          {title}
        </Text>
      ) : null}
      <VStack spacing={0} modifiers={[background(withAlpha(colors.content, 0.07), CARD_SHAPE)]}>
        {stats.map((stat, index) => (
          <Fragment key={stat.label}>
            {index > 0 ? (
              <Divider modifiers={[padding({ leading: 16 })]} />
            ) : null}
            <HStack modifiers={[padding({ horizontal: 16, vertical: 13 })]}>
              <Text modifiers={[SECONDARY]}>{stat.label}</Text>
              <Spacer />
              <Text modifiers={[font({ weight: "medium" }), monospacedDigit()]}>
                {stat.unit === "%"
                  ? `${stat.value}%`
                  : stat.unit
                    ? `${stat.value} ${stat.unit}`
                    : stat.value}
              </Text>
            </HStack>
          </Fragment>
        ))}
      </VStack>
    </VStack>
  );
}

const statValue = (stat: MetricStat) =>
  stat.unit === "%" ? `${stat.value}%` : stat.unit ? `${stat.value} ${stat.unit}` : stat.value;

function AndroidSheet({
  isPresented,
  onDismiss,
  highlights,
  tokens,
  completedAt,
}: {
  isPresented: boolean;
  onDismiss: () => void;
  highlights: MetricStat[];
  tokens: MetricStat[];
  completedAt?: number;
}) {
  const colors = useColors();
  const styles = useStyles();
  const { width } = useWindowDimensions();
  const { bottom } = useSafeAreaInsets();
  return (
    <BottomSheet
      isPresented={isPresented}
      onDismiss={onDismiss}
      contentPadding={0}
      containerColor={colors.sheet}
    >
      <RNHostView matchContents={{ vertical: true }}>
        <View style={[styles.sheet, { width, paddingBottom: Math.max(bottom, 12) + 12 }]}>
          <View style={styles.header}>
            <RNText accessibilityRole="header" style={styles.title}>Turn metrics</RNText>
            {completedAt != null ? (
              <RNText style={styles.subtitle}>{`Finished at ${formatClockTime(completedAt)}`}</RNText>
            ) : null}
          </View>
          {highlights.length > 0 ? <AndroidStatGroup stats={highlights} /> : null}
          {tokens.length > 0 ? <AndroidStatGroup title="Tokens" stats={tokens} /> : null}
        </View>
      </RNHostView>
    </BottomSheet>
  );
}

function AndroidStatGroup({ title, stats }: { title?: string; stats: MetricStat[] }) {
  const styles = useStyles();
  return (
    <View style={styles.group}>
      {title ? <RNText style={styles.groupTitle}>{title}</RNText> : null}
      <View style={styles.card}>
        {stats.map((stat, index) => (
          <Fragment key={stat.label}>
            {index > 0 ? <View style={styles.divider} /> : null}
            <View style={styles.row}>
              <RNText style={styles.label}>{stat.label}</RNText>
              <RNText style={styles.value}>{statValue(stat)}</RNText>
            </View>
          </Fragment>
        ))}
      </View>
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  sheet: { paddingTop: 28, paddingHorizontal: 20, gap: 20 },
  header: { gap: 2 },
  title: { color: colors.content, fontSize: 20, fontWeight: "600" },
  subtitle: { color: colors.muted, fontSize: 15 },
  group: { gap: 8 },
  groupTitle: { color: colors.muted, fontSize: 13, fontWeight: "600", paddingLeft: 4 },
  card: { backgroundColor: withAlpha(colors.content, 0.07), borderRadius: 20, overflow: "hidden" },
  divider: { height: StyleSheet.hairlineWidth, marginLeft: 16, backgroundColor: withAlpha(colors.content, 0.12) },
  row: { flexDirection: "row", justifyContent: "space-between", paddingHorizontal: 16, paddingVertical: 13 },
  label: { color: colors.muted, fontSize: 16 },
  value: { color: colors.content, fontSize: 16, fontWeight: "500", fontVariant: ["tabular-nums"] },
}));

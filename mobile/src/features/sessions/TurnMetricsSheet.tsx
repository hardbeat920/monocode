import { BottomSheet } from "@expo/ui";
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

import type { TurnMetrics } from "@/features/workspace/types";
import { withAlpha, useColors } from "@/ui/theme";

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

import { View } from "react-native";

import type { OrchestrationSummary } from "@/features/workspace/types";
import { Icon } from "@/ui/Icon";
import { themedStyles } from "@/ui/theme";

import { useToneColors } from "./tones";

/** A lead's agents on its session row: a dot per assignment, in its tone. */
export function AgentDots({ run }: { run: OrchestrationSummary }) {
  const styles = useStyles();
  const { accent, tone } = useToneColors();
  return (
    <View
      style={styles.dots}
      accessible
      accessibilityLabel={`Agents: ${run.headline}`}
    >
      <Icon name="orchestrator" size={12} color={accent} />
      {run.tones.map((value, index) => (
        <View key={index} style={[styles.dot, { backgroundColor: tone(value) }]} />
      ))}
    </View>
  );
}

const useStyles = themedStyles(() => ({
  dots: {
    flexDirection: "row",
    alignItems: "center",
    gap: 3,
  },
  dot: {
    width: 6,
    height: 6,
    borderRadius: 3,
  },
}));

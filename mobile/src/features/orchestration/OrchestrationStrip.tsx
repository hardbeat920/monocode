import { router } from "expo-router";
import { Pressable, Text, View } from "react-native";

import type { SessionSummary } from "@/features/workspace/types";
import { Icon } from "@/ui/Icon";
import { themedStyles, useColors } from "@/ui/theme";

import { useToneColors } from "./tones";

/**
 * The line under a session's header when it belongs to an Orchestrator run.
 * On a lead: a dot per assignment and the desktop card's headline, opening
 * the Agents sheet. On a worker: its lead, to go back to.
 */
export function OrchestrationStrip({
  session,
  lead,
}: {
  session: SessionSummary;
  /** The worker's lead, when `session` is a worker. */
  lead?: SessionSummary;
}) {
  const colors = useColors();
  const styles = useStyles();
  const { accent, tone } = useToneColors();
  const run = session.orchestration;

  if (lead) {
    return (
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={`Agent of ${lead.title}. Open the lead.`}
        hitSlop={{ top: 4, bottom: 4 }}
        onPress={() =>
          router.dismissTo({
            pathname: "/project/[projectId]/session/[sessionId]",
            params: { projectId: lead.projectId, sessionId: lead.id },
          })
        }
        style={({ pressed }) => [styles.trigger, pressed && styles.pressed]}
      >
        <Icon name="orchestrator" size={12} color={accent} />
        <Text style={styles.text} numberOfLines={1}>
          Agent of <Text style={styles.strong}>{lead.title}</Text>
        </Text>
        <Icon name="chevronRight" size={12} color={colors.muted} />
      </Pressable>
    );
  }
  if (!run) return null;
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={`Agents: ${run.title}, ${run.headline}`}
      hitSlop={{ top: 4, bottom: 4 }}
      onPress={() =>
        router.push({
          pathname: "/agents",
          params: { projectId: session.projectId, sessionId: session.id },
        })
      }
      style={({ pressed }) => [styles.trigger, pressed && styles.pressed]}
    >
      <Icon name="orchestrator" size={12} color={accent} />
      <View style={styles.dots}>
        {run.tones.map((value, index) => (
          <View
            key={index}
            style={[styles.dot, { backgroundColor: tone(value) }]}
          />
        ))}
      </View>
      <Text style={styles.text} numberOfLines={1}>
        {run.headline}
      </Text>
      <Icon name="chevronRight" size={12} color={colors.muted} />
    </Pressable>
  );
}

const useStyles = themedStyles((colors) => ({
  trigger: {
    flexShrink: 1,
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    paddingVertical: 4,
    paddingHorizontal: 8,
    borderRadius: 8,
  },
  pressed: {
    opacity: 0.6,
  },
  dots: {
    flexDirection: "row",
    gap: 3,
  },
  dot: {
    width: 6,
    height: 6,
    borderRadius: 3,
  },
  text: {
    flexShrink: 1,
    color: colors.muted,
    fontSize: 13,
  },
  strong: {
    color: colors.content,
    fontWeight: "500",
  },
}));

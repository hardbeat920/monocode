import { router } from "expo-router";
import { Pressable, Text } from "react-native";

import type { SessionSummary } from "@/features/workspace/types";
import { Icon } from "@/ui/Icon";
import { themedStyles, useColors } from "@/ui/theme";

/** The line under a subagent's header: its lead, to go back to. */
export function LeadLink({ lead }: { lead: SessionSummary }) {
  const colors = useColors();
  const styles = useStyles();
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
      <Icon name="subagent" size={12} color={colors.muted} />
      <Text style={styles.text} numberOfLines={1}>
        Agent of <Text style={styles.strong}>{lead.title}</Text>
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

import { router } from "expo-router";
import { Text, View } from "react-native";

import { useProjectGit } from "@/features/connection/useProjectGit";
import type { ChangeSummary } from "@/features/workspace/types";
import { Icon } from "@/ui/Icon";
import { LiquidView } from "@/ui/LiquidView";
import { radii, themedStyles, useColors } from "@/ui/theme";

import { PLAN_FROM_FILES, shipStep, shipTitle, type ShipStep } from "./commitPlan";
import { formatCount, useDiffTheme } from "./diffTheme";

const ACTION: Record<ShipStep, string> = {
  commit: "Plan commits",
  push: "Push",
  pr: "Open PR",
  done: "View",
};

/**
 * Where the session's work stands, above the composer while the agent is
 * idle: files to commit, commits to push, a pull request to open. A few files
 * go to the commit sheet, more to the commit plan. Gone when there is nothing
 * to ship, and once the pull request is open.
 */
export function ShipCard({
  projectId,
  worktreePath,
  harness,
  summary,
  enabled,
}: {
  projectId: string;
  worktreePath?: string;
  harness?: string;
  summary: ChangeSummary;
  /** Read the branch only while the agent is idle and there is a card to show. */
  enabled: boolean;
}) {
  const colors = useColors();
  const diff = useDiffTheme();
  const styles = useStyles();
  const git = useProjectGit(projectId, enabled, worktreePath, harness);
  const step = shipStep(summary.files, git.status);
  if (!enabled || !step || step === "done") return null;

  const small = step === "commit" && summary.files < PLAN_FROM_FILES;
  const open = () =>
    router.push({
      pathname: small ? "/changes" : "/commit-plan",
      params: {
        projectId,
        ...(worktreePath ? { worktreePath } : {}),
        ...(harness ? { harness } : {}),
      },
    });
  const { additions, deletions } = summary;

  const title = shipTitle(step, summary.files, git.status);
  return (
    <LiquidView
      borderRadius={radii.pill}
      innerStyle={styles.card}
      style={styles.slot}
      onPress={open}
      pressedScale={1.02}
      accessibilityLabel={`${title}. ${small ? "Commit" : ACTION[step]}`}
    >
      <Icon name="branch" size={16} color={colors.muted} />
      <View style={styles.text}>
        <Text style={styles.title} numberOfLines={1}>
          {title}
        </Text>
        {step === "commit" ? (
          <Text style={styles.counts}>
            <Text style={{ color: diff.add.fg }}>+{formatCount(additions)}</Text>{" "}
            <Text style={{ color: diff.del.fg }}>-{formatCount(deletions)}</Text>
          </Text>
        ) : null}
      </View>
      <View style={styles.action}>
        <Text style={styles.actionLabel}>{small ? "Commit" : ACTION[step]}</Text>
      </View>
    </LiquidView>
  );
}

const useStyles = themedStyles((colors) => ({
  // The glass is the app's own, as the working-agent pills above the composer.
  slot: { marginHorizontal: 16, marginBottom: 8 },
  card: {
    gap: 10,
    paddingVertical: 8,
    paddingLeft: 14,
    paddingRight: 8,
  },
  text: { flex: 1, gap: 1 },
  title: { color: colors.content, fontSize: 14, fontWeight: "600" },
  counts: { color: colors.muted, fontSize: 12, fontVariant: ["tabular-nums"] },
  action: {
    height: 34,
    paddingHorizontal: 14,
    borderRadius: 17,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.inverseSurface,
  },
  actionLabel: { color: colors.inverse, fontSize: 14, fontWeight: "600" },
}));

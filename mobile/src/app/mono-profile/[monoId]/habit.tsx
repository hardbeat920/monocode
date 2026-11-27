import { Stack, useLocalSearchParams } from "expo-router";
import { Fragment } from "react";
import { ActivityIndicator, StyleSheet, Text, View } from "react-native";

import { Markdown } from "@/features/sessions/Markdown";
import { SettingsPage } from "@/features/settings/SettingsUI";
import { useMonoProfile } from "@/features/workspace/monoProfile";
import { radii, themedStyles, useColors } from "@/ui/theme";

// A habit, pushed inside a Mono's profile as the desktop opens it beside the
// list: when it runs and whether it's on, what it's told to do, and how its
// latest runs went.
export default function HabitScreen() {
  const { monoId, habitId } = useLocalSearchParams<{ monoId: string; habitId: string }>();
  const { profile, error, refreshing, refresh } = useMonoProfile(monoId);
  const styles = useStyles();
  const colors = useColors();
  const habit = profile?.habits.find((h) => h.id === habitId);
  return (
    <SettingsPage onRefresh={refresh} refreshing={refreshing}>
      <Stack.Screen options={{ title: habit?.name ?? "Habit" }} />
      {!profile ? (
        error ? (
          <Text style={styles.note}>{error}</Text>
        ) : (
          <ActivityIndicator color={colors.muted} style={styles.loading} />
        )
      ) : !habit ? (
        <Text style={styles.note}>This habit is no longer there.</Text>
      ) : (
        <>
          <View style={styles.card}>
            <Line label="Schedule" value={habit.schedule} />
            <View style={styles.divider} />
            <Line label={habit.running ? "Running" : "Next run"} value={habit.enabled || habit.running ? habit.when : "Paused"} />
            {habit.lastOutcome ? (
              <>
                <View style={styles.divider} />
                <Line label="Last run" value={habit.lastOutcome} />
              </>
            ) : null}
          </View>
          {habit.lastError ? <Text style={styles.error}>{habit.lastError}</Text> : null}

          <Text style={styles.heading}>Instructions</Text>
          <View style={[styles.card, styles.padded]}>
            <Markdown text={habit.instructions} />
          </View>

          <Text style={styles.heading}>Recent runs</Text>
          <View style={styles.card}>
            {habit.runs.length ? (
              habit.runs.map((run, index) => (
                <Fragment key={run.at}>
                  {index > 0 ? <View style={styles.divider} /> : null}
                  <View style={styles.run}>
                    <View style={styles.runHead}>
                      <Text style={styles.runTitle}>{new Date(run.at).toLocaleString([], { dateStyle: "medium", timeStyle: "short" })}</Text>
                      <Text style={styles.runDetail}>{run.outcome}{run.durationMs ? ` · ${duration(run.durationMs)}` : ""}</Text>
                    </View>
                    {run.error ? <Text style={styles.runError} numberOfLines={3}>{run.error}</Text> : null}
                    {run.report ? <Text style={styles.runDetail} numberOfLines={4}>{run.report}</Text> : null}
                  </View>
                </Fragment>
              ))
            ) : (
              <Text style={styles.empty}>It hasn&apos;t run yet.</Text>
            )}
          </View>
        </>
      )}
    </SettingsPage>
  );
}

function Line({ label, value }: { label: string; value: string }) {
  const styles = useStyles();
  return (
    <View style={styles.line}>
      <Text style={styles.lineLabel}>{label}</Text>
      <Text style={styles.lineValue} numberOfLines={1}>{value}</Text>
    </View>
  );
}

function duration(ms: number) {
  const seconds = Math.round(ms / 1000);
  return seconds < 60 ? `${seconds}s` : `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
}

const useStyles = themedStyles((colors) => ({
  loading: { marginTop: 32 },
  note: {
    color: colors.muted,
    fontSize: 14,
    marginTop: 24,
    textAlign: "center",
  },
  heading: {
    color: colors.muted,
    fontSize: 15,
    marginTop: 24,
    marginBottom: 8,
    marginLeft: 16,
  },
  card: {
    backgroundColor: colors.surface,
    borderRadius: radii.card,
    borderCurve: "continuous",
    paddingHorizontal: 10,
    paddingVertical: 6,
  },
  padded: {
    paddingHorizontal: 16,
    paddingVertical: 12,
  },
  divider: {
    height: StyleSheet.hairlineWidth,
    marginLeft: 8,
    backgroundColor: colors.stroke,
  },
  line: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
    paddingHorizontal: 8,
    paddingVertical: 12,
  },
  lineLabel: {
    color: colors.content,
    fontSize: 16,
  },
  lineValue: {
    flex: 1,
    textAlign: "right",
    color: colors.muted,
    fontSize: 16,
  },
  error: {
    color: colors.deletions,
    fontSize: 13,
    lineHeight: 18,
    marginTop: 8,
    marginHorizontal: 16,
  },
  run: {
    gap: 4,
    paddingHorizontal: 8,
    paddingVertical: 11,
  },
  runHead: {
    flexDirection: "row",
    alignItems: "baseline",
    gap: 8,
  },
  runTitle: {
    flex: 1,
    color: colors.content,
    fontSize: 15,
  },
  runDetail: {
    color: colors.muted,
    fontSize: 13,
    lineHeight: 18,
  },
  runError: {
    color: colors.deletions,
    fontSize: 13,
    lineHeight: 18,
  },
  empty: {
    color: colors.muted,
    fontSize: 14,
    paddingHorizontal: 8,
    paddingVertical: 12,
  },
}));

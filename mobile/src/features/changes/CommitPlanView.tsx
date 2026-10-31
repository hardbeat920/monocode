import * as Haptics from "expo-haptics";
import { useEffect, useState } from "react";
import {
  ActivityIndicator,
  Alert,
  Linking,
  Pressable,
  ScrollView,
  Text,
  TextInput,
  View,
} from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { useProjectGit, type PlannedCommit } from "@/features/connection/useProjectGit";
import { Icon } from "@/ui/Icon";
import { withAlpha, themedStyles, useColors, useScheme } from "@/ui/theme";

import { selection, shipStep, shipTitle, syncLabel } from "./commitPlan";
import { MONO, formatCount, useDiffTheme } from "./diffTheme";

/** Files a commit lists when opened, before "+ N more". */
const PREVIEW = 6;
const BAR = 50;

type Busy = "commit" | "push" | "pr";

/**
 * The commit plan: the desktop's model splits every changed file into focused
 * commits, each of which can be skipped, reworded or opened up to its files;
 * then the same button carries the work on through push and pull request.
 * Where the work stands is read from the branch, not remembered, so closing
 * the modal half way and coming back finds the next step waiting.
 */
export function CommitPlanView({
  projectId,
  worktreePath,
  harness,
  files,
  onChanged,
}: {
  projectId: string;
  worktreePath?: string;
  harness?: string;
  /** Changed files in the working copy, staged or not. */
  files: number;
  /** After anything that changed the repository, to read it again. */
  onChanged: () => void;
}) {
  const colors = useColors();
  const scheme = useScheme();
  const diff = useDiffTheme();
  const styles = useStyles();
  const insets = useSafeAreaInsets();
  const git = useProjectGit(projectId, true, worktreePath, harness);
  const { run, reload } = git;

  const [plan, setPlan] = useState<PlannedCommit[] | null>(null);
  const [on, setOn] = useState<boolean[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<number | null>(null);
  const [open, setOpen] = useState<number | null>(null);
  const [busy, setBusy] = useState<Busy | null>(null);
  // Bumped to ask for a new plan; one still being written is dropped.
  const [attempt, setAttempt] = useState(0);

  const step = shipStep(files, git.status);
  const planning = step === "commit" && !plan && !error;

  useEffect(() => {
    if (files === 0) return;
    let disposed = false;
    run({ action: "plan" })
      .then((result) => {
        if (disposed) return;
        const commits = result.commits ?? [];
        setPlan(commits);
        setOn(commits.map(() => true));
      })
      .catch((reason) => {
        if (!disposed) setError(reason instanceof Error ? reason.message : String(reason));
      });
    return () => {
      disposed = true;
    };
    // `run` is new each render; a plan is asked for once per attempt, and
    // again only if the working copy goes from clean to changed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [attempt, files > 0]);

  // A new plan for what is left, dropping the one shown.
  const replan = () => {
    setPlan(null);
    setError(null);
    setAttempt((n) => n + 1);
  };

  const kept = plan ? selection(plan, on) : null;
  const status = git.status;

  const fail = (title: string, reason: unknown) =>
    Alert.alert(title, reason instanceof Error ? reason.message : String(reason));

  const commit = async () => {
    if (!plan || !kept || kept.commits.length === 0 || busy) return;
    setBusy("commit");
    try {
      await run({
        action: "commitPlan",
        commits: kept.commits.map(({ message, paths }) => ({ message, paths })),
      });
      void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
      // What was skipped is still to commit, so it stays as the plan.
      const skipped = plan.filter((_, i) => !on[i]);
      setPlan(skipped);
      setOn(skipped.map(() => true));
      setOpen(null);
    } catch (reason) {
      fail("Couldn't make the commits", reason);
      // Some may have been made before it failed, so plan what is left.
      replan();
    } finally {
      setBusy(null);
      onChanged();
      reload();
    }
  };

  const push = async () => {
    if (busy) return;
    setBusy("push");
    try {
      await run({ action: "sync" });
      void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
    } catch (reason) {
      fail("Couldn't push", reason);
    } finally {
      setBusy(null);
      onChanged();
      reload();
    }
  };

  const openPr = async () => {
    if (busy) return;
    setBusy("pr");
    try {
      const { url } = await run({ action: "pr" });
      void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
      if (url) void Linking.openURL(url);
    } catch (reason) {
      fail("Couldn't create the pull request", reason);
    } finally {
      setBusy(null);
      onChanged();
      reload();
    }
  };

  const update = (index: number, change: Partial<PlannedCommit>) =>
    setPlan((current) =>
      current ? current.map((c, i) => (i === index ? { ...c, ...change } : c)) : current,
    );

  const toggle = (index: number) => {
    void Haptics.selectionAsync();
    setOn((current) => current.map((value, i) => (i === index ? !value : value)));
  };

  const action = (() => {
    if (step === "commit")
      return {
        label: kept
          ? `Commit ${kept.commits.length} · ${formatCount(kept.files)} ${kept.files === 1 ? "file" : "files"}`
          : "Planning…",
        enabled: !!kept && kept.commits.length > 0 && !busy,
        busy: busy === "commit",
        onPress: () => void commit(),
      };
    if (step === "push")
      return {
        label: status ? syncLabel(status) : "Push",
        enabled: !busy,
        busy: busy === "push",
        onPress: () => void push(),
      };
    if (step === "pr")
      return {
        label: "Open pull request",
        enabled: !busy,
        busy: busy === "pr",
        onPress: () => void openPr(),
      };
    if (step === "done" && status?.pr) {
      const url = status.pr.url;
      return {
        label: `View PR #${status.pr.number}`,
        enabled: true,
        busy: false,
        onPress: () => void Linking.openURL(url),
      };
    }
    return null;
  })();

  const summary = (() => {
    if (step === "commit")
      return plan?.length
        ? `${formatCount(files)} ${files === 1 ? "file" : "files"} in ${plan.length} ${plan.length === 1 ? "commit" : "commits"}. Keep, reword or skip each one.`
        : error
          ? null
          : "Claude is grouping your changes into commits…";
    if (step) return `${shipTitle(step, files, status)}.`;
    return status ? "Nothing to commit or push." : "Reading the branch from desktop…";
  })();

  return (
    <View style={styles.root}>
      <ScrollView
        contentContainerStyle={[styles.content, { paddingBottom: BAR + insets.bottom + 40 }]}
        keyboardShouldPersistTaps="handled"
        contentInsetAdjustmentBehavior="automatic"
      >
        {summary ? <Text style={styles.summary}>{summary}</Text> : null}

        {planning ? (
          <View style={styles.status}>
            <ActivityIndicator color={colors.muted} />
          </View>
        ) : null}

        {error && step === "commit" ? (
          <View style={styles.status}>
            <Text style={styles.problem}>{updateHint(error)}</Text>
            <Pressable
              accessibilityRole="button"
              onPress={replan}
              style={({ pressed }) => [styles.retry, pressed && styles.pressed]}
            >
              <Icon name="refresh" size={16} />
              <Text style={styles.retryLabel}>Try again</Text>
            </Pressable>
          </View>
        ) : null}

        {step === "commit" && plan
          ? plan.map((item, index) => {
              const wanted = on[index] ?? true;
              const last = index === plan.length - 1;
              const expanded = open === index;
              return (
                <View key={index} style={styles.commit}>
                  <View style={styles.rail}>
                    <Pressable
                      accessibilityRole="checkbox"
                      accessibilityLabel={`Commit ${index + 1}`}
                      accessibilityState={{ checked: wanted }}
                      hitSlop={10}
                      onPress={() => toggle(index)}
                      style={[styles.check, wanted && styles.checkOn]}
                    >
                      {wanted ? <Icon name="check" size={12} color={colors.inverse} /> : null}
                    </Pressable>
                    {last ? null : <View style={styles.line} />}
                  </View>
                  <View style={[styles.card, !wanted && styles.skipped]}>
                    {editing === index ? (
                      <TextInput
                        autoFocus
                        multiline
                        value={item.message}
                        onChangeText={(message) => update(index, { message })}
                        onBlur={() => setEditing(null)}
                        keyboardAppearance={scheme}
                        cursorColor={colors.accent}
                        selectionColor={colors.accent}
                        style={styles.input}
                      />
                    ) : (
                      <View style={styles.titleRow}>
                        <Pressable
                          style={styles.flex}
                          onPress={() => setOpen(expanded ? null : index)}
                        >
                          <Text style={styles.message}>{item.message}</Text>
                        </Pressable>
                        <Pressable
                          accessibilityRole="button"
                          accessibilityLabel="Edit message"
                          hitSlop={10}
                          onPress={() => setEditing(index)}
                        >
                          <Icon name="edit" size={15} color={colors.muted} />
                        </Pressable>
                      </View>
                    )}
                    <Pressable
                      style={styles.meta}
                      onPress={() => setOpen(expanded ? null : index)}
                    >
                      <Text style={styles.count}>
                        {formatCount(item.paths.length)} {item.paths.length === 1 ? "file" : "files"}
                      </Text>
                      <Text style={styles.count}>
                        <Text style={{ color: diff.add.fg }}>+{formatCount(item.additions)}</Text>{" "}
                        <Text style={{ color: diff.del.fg }}>-{formatCount(item.deletions)}</Text>
                      </Text>
                    </Pressable>
                    {expanded ? (
                      <View style={styles.paths}>
                        {item.paths.slice(0, PREVIEW).map((path) => (
                          <Text key={path} style={styles.path} numberOfLines={1} ellipsizeMode="head">
                            {path}
                          </Text>
                        ))}
                        {item.paths.length > PREVIEW ? (
                          <Text style={styles.count}>+ {item.paths.length - PREVIEW} more</Text>
                        ) : null}
                      </View>
                    ) : null}
                  </View>
                </View>
              );
            })
          : null}
      </ScrollView>

      {action ? (
        <View style={[styles.bar, { paddingBottom: Math.max(insets.bottom, 12) }]}>
          <Pressable
            accessibilityRole="button"
            disabled={!action.enabled}
            onPress={action.onPress}
            style={({ pressed }) => [
              styles.button,
              !action.enabled && !action.busy && styles.dimmed,
              pressed && styles.pressed,
            ]}
          >
            {action.busy ? (
              <ActivityIndicator size="small" color={colors.inverse} />
            ) : (
              <Icon name={step === "commit" ? "check" : "send"} size={18} color={colors.inverse} />
            )}
            <Text style={styles.buttonLabel}>{action.label}</Text>
          </Pressable>
        </View>
      ) : null}
    </View>
  );
}

/** An older desktop doesn't know the plan request; say what fixes it. */
function updateHint(error: string) {
  return /unsupported/i.test(error)
    ? "Update Monocode on your desktop to plan commits from here."
    : error;
}

const useStyles = themedStyles((colors) => ({
  root: { flex: 1 },
  flex: { flex: 1 },
  content: { paddingHorizontal: 16, paddingTop: 8, gap: 0 },
  summary: {
    color: colors.muted,
    fontSize: 14,
    lineHeight: 20,
    paddingHorizontal: 4,
    paddingBottom: 14,
  },
  status: { alignItems: "center", gap: 14, paddingVertical: 32 },
  problem: { color: colors.muted, fontSize: 14, textAlign: "center" },
  retry: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
    height: 40,
    paddingHorizontal: 18,
    borderRadius: 20,
    backgroundColor: withAlpha(colors.content, 0.07),
  },
  retryLabel: { color: colors.content, fontSize: 15, fontWeight: "600" },
  commit: { flexDirection: "row", gap: 10 },
  rail: { width: 22, alignItems: "center", paddingTop: 14 },
  check: {
    width: 22,
    height: 22,
    borderRadius: 11,
    borderWidth: 1.5,
    borderColor: colors.faint,
    alignItems: "center",
    justifyContent: "center",
  },
  checkOn: { backgroundColor: colors.inverseSurface, borderColor: colors.inverseSurface },
  line: { flex: 1, width: 2, marginTop: 4, marginBottom: -4, backgroundColor: colors.stroke },
  card: {
    flex: 1,
    marginBottom: 10,
    padding: 14,
    gap: 6,
    borderRadius: 18,
    borderCurve: "continuous",
    backgroundColor: withAlpha(colors.content, 0.07),
  },
  skipped: { opacity: 0.45 },
  titleRow: { flexDirection: "row", alignItems: "flex-start", gap: 10 },
  message: { color: colors.content, fontSize: 16, lineHeight: 21, fontWeight: "500" },
  input: {
    color: colors.content,
    fontSize: 16,
    lineHeight: 21,
    padding: 0,
    textAlignVertical: "top",
  },
  meta: { flexDirection: "row", justifyContent: "space-between" },
  count: { color: colors.muted, fontSize: 12, fontVariant: ["tabular-nums"] },
  paths: { gap: 3, paddingTop: 4 },
  path: { color: colors.muted, fontFamily: MONO, fontSize: 12 },
  bar: {
    position: "absolute",
    left: 0,
    right: 0,
    bottom: 0,
    paddingTop: 10,
    paddingHorizontal: 16,
    backgroundColor: colors.background,
    borderTopWidth: 1,
    borderTopColor: colors.stroke,
  },
  button: {
    height: BAR,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
    gap: 8,
    borderRadius: BAR / 2,
    backgroundColor: colors.inverseSurface,
  },
  buttonLabel: { color: colors.inverse, fontSize: 16, fontWeight: "600" },
  dimmed: { opacity: 0.4 },
  pressed: { opacity: 0.7 },
}));

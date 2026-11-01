import * as Haptics from "expo-haptics";
import { router } from "expo-router";
import { useCallback, useEffect, useState } from "react";
import { ActivityIndicator, Alert, AppState, Pressable, Text, View } from "react-native";

import { SettingsPage } from "@/features/settings/SettingsUI";
import { ProviderLogo } from "@/features/sessions/ProviderLogo";
import type {
  Harness,
  OrchestrationActionName,
  OrchestrationRun,
  OrchestrationTask,
} from "@/features/workspace/types";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { Icon } from "@/ui/Icon";
import { radii, themedStyles, useColors, withAlpha } from "@/ui/theme";

import { useToneColors } from "./tones";

/** How often an open sheet follows the run. The desktop card refreshes once a second. */
const POLL_MS = 2000;

const failed = (reason: unknown) =>
  Alert.alert(
    "Could not update the agents",
    reason instanceof Error ? reason.message : String(reason),
  );

/**
 * A lead's Orchestrator cards, as on the desktop: the plan to review and
 * start, then each agent's progress, its assignment and report, and its chat.
 * The desktop decides which buttons apply; this only presses them.
 */
export function AgentsSheet({
  projectId,
  sessionId,
}: {
  projectId: string;
  sessionId: string;
}) {
  const { readOrchestration, orchestrationAction } = useWorkspace();
  const styles = useStyles();
  const colors = useColors();
  const [runs, setRuns] = useState<OrchestrationRun[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      setRuns(await readOrchestration(sessionId));
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, [readOrchestration, sessionId]);
  // Read at once, then follow the run while the sheet is open and in view.
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      if (AppState.currentState === "active") await load();
      if (!disposed) timer = setTimeout(poll, POLL_MS);
    };
    void poll();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [load]);

  const press = (
    run: OrchestrationRun,
    action: OrchestrationActionName,
    task?: OrchestrationTask,
  ) => {
    const go = () => {
      void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light);
      setBusy(true);
      orchestrationAction(sessionId, run, action, task)
        .then(setRuns, failed)
        .finally(() => setBusy(false));
    };
    const ask = (title: string, message: string, label: string) =>
      Alert.alert(title, message, [
        { text: "Cancel", style: "cancel" },
        { text: label, style: "destructive", onPress: go },
      ]);
    if (action === "stop")
      return ask("Stop the agents?", "Agents still working are stopped. You can resume later.", "Stop");
    if (action === "dismiss")
      return ask("Dismiss this plan?", "No agents start. Ask the lead for a new plan any time.", "Dismiss");
    if (action === "remove" && task)
      return ask(`Remove “${task.title}”?`, "It leaves the plan, along with anything waiting on it.", "Remove");
    go();
  };

  const openAgent = (task: OrchestrationTask) => {
    if (!task.sessionId) return;
    router.back();
    router.push({
      pathname: "/project/[projectId]/session/[sessionId]",
      params: { projectId, sessionId: task.sessionId },
    });
  };

  if (!runs)
    return (
      <SettingsPage>
        <View style={styles.center}>
          {error ? (
            <Text style={styles.muted}>{error}</Text>
          ) : (
            <ActivityIndicator color={colors.muted} />
          )}
        </View>
      </SettingsPage>
    );
  if (!runs.length)
    return (
      <SettingsPage>
        <View style={styles.center}>
          <Text style={styles.muted}>
            No plan right now. Send a message in Orchestrator mode to get one.
          </Text>
        </View>
      </SettingsPage>
    );
  return (
    <SettingsPage onRefresh={() => void load()}>
      {runs.map((run) => (
        <RunCard
          key={run.call || run.title}
          run={run}
          busy={busy}
          onAction={(action, task) => press(run, action, task)}
          onOpen={openAgent}
        />
      ))}
    </SettingsPage>
  );
}

function RunCard({
  run,
  busy,
  onAction,
  onOpen,
}: {
  run: OrchestrationRun;
  busy: boolean;
  onAction: (action: OrchestrationActionName, task?: OrchestrationTask) => void;
  onOpen: (task: OrchestrationTask) => void;
}) {
  const styles = useStyles();
  const colors = useColors();
  const { accent, tone } = useToneColors();
  const reviewing = run.phase === "review";
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());
  const toggle = (id: string) =>
    setOpen((previous) => {
      const next = new Set(previous);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  return (
    <View style={styles.run}>
      <View style={styles.kicker}>
        <Icon name="orchestrator" size={13} color={accent} />
        <Text style={[styles.kickerText, { color: accent }]} numberOfLines={1}>
          {run.headline}
        </Text>
      </View>
      <Text style={styles.title}>{run.title}</Text>
      {run.summary ? <Text style={styles.summary}>{run.summary}</Text> : null}
      {reviewing ? null : (
        <View style={styles.progress}>
          {run.tasks.map((task) => (
            <View
              key={task.id}
              style={[
                styles.segment,
                {
                  backgroundColor:
                    task.tone === "queued" ? colors.rail : tone(task.tone),
                },
              ]}
            />
          ))}
        </View>
      )}
      {run.reason ? <Text style={styles.reason}>{run.reason}</Text> : null}
      {run.actions.length ? (
        <View style={styles.actions}>
          {run.actions.map((button) => (
            <Pressable
              key={button.action}
              accessibilityRole="button"
              accessibilityState={{ disabled: button.disabled || busy }}
              disabled={button.disabled || busy}
              onPress={() => onAction(button.action)}
              style={({ pressed }) => [
                styles.button,
                button.primary && styles.primary,
                (button.disabled || busy) && styles.disabled,
                pressed && styles.pressed,
              ]}
            >
              <Text
                style={[styles.buttonText, button.primary && styles.primaryText]}
              >
                {button.label}
              </Text>
            </Pressable>
          ))}
        </View>
      ) : null}

      <View style={styles.tasks}>
        {run.tasks.map((task) => (
          <TaskRow
            key={task.id}
            task={task}
            reviewing={reviewing}
            open={open.has(task.id)}
            busy={busy}
            onToggle={() => toggle(task.id)}
            onOpen={() => onOpen(task)}
            onRetry={() => onAction("retry", task)}
            onRemove={() => onAction("remove", task)}
          />
        ))}
      </View>
      {reviewing ? (
        <Text style={styles.footnote}>
          Reply in the chat to revise the plan. Agents share this checkout;
          overlapping scopes run one after another.
        </Text>
      ) : null}
    </View>
  );
}

function TaskRow({
  task,
  reviewing,
  open,
  busy,
  onToggle,
  onOpen,
  onRetry,
  onRemove,
}: {
  task: OrchestrationTask;
  reviewing: boolean;
  open: boolean;
  busy: boolean;
  onToggle: () => void;
  onOpen: () => void;
  onRetry: () => void;
  onRemove: () => void;
}) {
  const styles = useStyles();
  const colors = useColors();
  const { tone } = useToneColors();
  const color = reviewing ? colors.faint : tone(task.tone);
  const meta = [
    task.model,
    task.steps > 0 ? `${task.steps} step${task.steps === 1 ? "" : "s"}` : null,
    task.note,
  ]
    .filter(Boolean)
    .join(" · ");
  const details: [string, string][] = [["Scope", task.files.join(", ")]];
  if (task.dependsOn.length) details.push(["After", task.dependsOn.join(", ")]);
  if (open) details.push(["Assignment", task.prompt]);
  if (open && task.report.trim()) details.push(["Report", task.report]);
  return (
    <View style={task.index > 0 && styles.task}>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={`${open ? "Hide" : "Show"} details for ${task.title}`}
        accessibilityState={{ expanded: open }}
        onPress={onToggle}
        style={({ pressed }) => [styles.taskHead, pressed && styles.pressed]}
      >
        <View style={[styles.dot, { backgroundColor: color }]} />
        <View style={styles.taskText}>
          <Text style={styles.taskTitle} numberOfLines={open ? undefined : 1}>
            {task.title}
          </Text>
          <View style={styles.metaLine}>
            <ProviderLogo harness={task.harness as Harness} size={12} color={colors.muted} />
            <Text style={styles.meta} numberOfLines={open ? undefined : 1}>
              {meta}
            </Text>
          </View>
        </View>
        {reviewing ? null : (
          <View
            style={[
              styles.label,
              { backgroundColor: withAlpha(color, 0.12), borderColor: withAlpha(color, 0.3) },
            ]}
          >
            <Text style={[styles.labelText, { color }]}>{task.label}</Text>
          </View>
        )}
        <Icon
          name={open ? "chevronDown" : "chevronRight"}
          size={12}
          color={colors.faint}
        />
      </Pressable>
      {open || task.outsideScope.length ? (
        <View style={styles.details}>
          {details.map(([name, text]) => (
            <View key={name}>
              <Text style={styles.detailName}>{name}</Text>
              <Text style={styles.detailText} selectable>
                {text}
              </Text>
            </View>
          ))}
          {task.outsideScope.length ? (
            <View>
              <Text style={[styles.detailName, { color: colors.caution }]}>
                Outside scope
              </Text>
              <Text style={styles.detailText}>{task.outsideScope.join(", ")}</Text>
            </View>
          ) : null}
          {open && (task.sessionId || task.canRetry || task.canRemove) ? (
            <View style={styles.actions}>
              {task.sessionId ? (
                <SmallButton label="Open chat" onPress={onOpen} />
              ) : null}
              {task.canRetry ? (
                <SmallButton label="Retry" onPress={onRetry} disabled={busy} />
              ) : null}
              {task.canRemove ? (
                <SmallButton label="Remove" onPress={onRemove} disabled={busy} />
              ) : null}
            </View>
          ) : null}
        </View>
      ) : null}
    </View>
  );
}

function SmallButton({
  label,
  onPress,
  disabled,
}: {
  label: string;
  onPress: () => void;
  disabled?: boolean;
}) {
  const styles = useStyles();
  return (
    <Pressable
      accessibilityRole="button"
      disabled={disabled}
      onPress={onPress}
      style={({ pressed }) => [
        styles.button,
        disabled && styles.disabled,
        pressed && styles.pressed,
      ]}
    >
      <Text style={styles.buttonText}>{label}</Text>
    </Pressable>
  );
}

const useStyles = themedStyles((colors) => ({
  center: {
    paddingVertical: 48,
    alignItems: "center",
  },
  muted: {
    color: colors.muted,
    fontSize: 15,
    textAlign: "center",
  },
  run: {
    marginBottom: 28,
  },
  kicker: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  kickerText: {
    flexShrink: 1,
    fontSize: 13,
    fontWeight: "600",
  },
  title: {
    marginTop: 6,
    color: colors.content,
    fontSize: 22,
    fontWeight: "700",
  },
  summary: {
    marginTop: 6,
    color: colors.muted,
    fontSize: 15,
    lineHeight: 21,
  },
  progress: {
    flexDirection: "row",
    gap: 4,
    marginTop: 14,
  },
  segment: {
    flex: 1,
    height: 4,
    borderRadius: 2,
  },
  reason: {
    marginTop: 12,
    color: colors.caution,
    fontSize: 14,
  },
  actions: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: 8,
    marginTop: 14,
  },
  button: {
    minHeight: 40,
    paddingHorizontal: 16,
    borderRadius: radii.pill,
    backgroundColor: colors.fill,
    alignItems: "center",
    justifyContent: "center",
  },
  primary: {
    backgroundColor: colors.inverseSurface,
  },
  disabled: {
    opacity: 0.45,
  },
  pressed: {
    opacity: 0.6,
  },
  buttonText: {
    color: colors.content,
    fontSize: 15,
    fontWeight: "600",
  },
  primaryText: {
    color: colors.inverse,
  },
  tasks: {
    marginTop: 18,
    borderRadius: radii.row,
    backgroundColor: colors.listCard,
    overflow: "hidden",
  },
  task: {
    borderTopWidth: 0.5,
    borderTopColor: colors.listDivider,
  },
  taskHead: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    paddingHorizontal: 14,
    paddingVertical: 12,
    minHeight: 56,
  },
  dot: {
    width: 8,
    height: 8,
    borderRadius: 4,
  },
  taskText: {
    flex: 1,
    minWidth: 0,
  },
  taskTitle: {
    color: colors.content,
    fontSize: 15,
    fontWeight: "500",
  },
  metaLine: {
    flexDirection: "row",
    alignItems: "center",
    gap: 5,
    marginTop: 3,
  },
  meta: {
    flexShrink: 1,
    color: colors.muted,
    fontSize: 13,
  },
  label: {
    paddingHorizontal: 8,
    paddingVertical: 2,
    borderRadius: radii.pill,
    borderWidth: 0.5,
  },
  labelText: {
    fontSize: 12,
    fontWeight: "600",
  },
  details: {
    gap: 10,
    paddingLeft: 32,
    paddingRight: 14,
    paddingBottom: 14,
  },
  detailName: {
    color: colors.muted,
    fontSize: 12,
    fontWeight: "600",
  },
  detailText: {
    marginTop: 2,
    color: colors.content,
    fontSize: 14,
    lineHeight: 20,
  },
  footnote: {
    marginTop: 12,
    color: colors.muted,
    fontSize: 13,
    lineHeight: 18,
  },
}));

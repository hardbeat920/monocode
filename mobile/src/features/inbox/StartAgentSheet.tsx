import * as Haptics from "expo-haptics";
import { router, Stack } from "expo-router";
import { useState, type ReactNode } from "react";
import { ActivityIndicator, Alert, Pressable, Text, TextInput, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { useConnection } from "@/features/connection/ConnectionProvider";
import { ProjectMascot } from "@/features/mascots/ProjectMascot";
import { useModelCatalog } from "@/features/models/useModelCatalog";
import { ProviderLogo } from "@/features/sessions/ProviderLogo";
import { markStartedHere } from "@/features/sessions/activityStore";
import { SettingsPage } from "@/features/settings/SettingsUI";
import { projectColor } from "@/features/workspace/projectColors";
import type { Harness } from "@/features/workspace/types";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { Icon } from "@/ui/Icon";
import { themedStyles, useColors, useScheme, withAlpha } from "@/ui/theme";

import { useInboxDetail } from "./detail";
import { FIXTURES_ENABLED } from "./fixtures";
import { useInbox } from "./InboxProvider";
import { ProviderMark } from "./ProviderMark";
import { defaultProject, startParams, type StartMode } from "./startAgent";

/**
 * "Start agent on this": pick the project, the agent and model, and whether
 * it works or only answers, add instructions if any, and the desktop starts
 * the session from the item (it writes the prompt). The new session opens.
 */
export function StartAgentSheet({
  itemKey,
  presetText,
}: {
  itemKey: string;
  presetText?: string;
}) {
  const colors = useColors();
  const scheme = useScheme();
  const styles = useStyles();
  const { bottom } = useSafeAreaInsets();
  const { client } = useInbox();
  const { allProjects } = useWorkspace();
  const connection = useConnection();
  const { item } = useInboxDetail(itemKey);

  const [chosenProject, setChosenProject] = useState<string | undefined>();
  const projectId = chosenProject ?? defaultProject(allProjects, item?.projectId, undefined);
  const project = allProjects.find((entry) => entry.id === projectId);
  const { catalog } = useModelCatalog(projectId);

  const [open, setOpen] = useState<"project" | "agent" | "model" | null>(null);
  const [harness, setHarness] = useState<Harness | undefined>();
  const [model, setModel] = useState<string | undefined>();
  const [mode, setMode] = useState<StartMode>("work");
  const [text, setText] = useState(presetText ?? "");
  const [starting, setStarting] = useState(false);

  const currentHarness = harness ?? catalog?.current.harness ?? project?.defaultHarness;
  const models = catalog?.models.filter((entry) => entry.harness === currentHarness) ?? [];
  const currentModel =
    model && models.some((entry) => entry.id === model)
      ? model
      : catalog && catalog.current.harness === currentHarness
        ? catalog.current.model
        : undefined;
  const modelName =
    models.find((entry) => entry.id === currentModel)?.name ?? project?.defaultModelName ?? "Default";
  const reachable = connection.connected || FIXTURES_ENABLED;

  const start = async () => {
    if (!item || !projectId || starting) return;
    void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Medium);
    setStarting(true);
    try {
      const { sessionId } = await client.start(
        startParams({
          key: item.key,
          projectId,
          mode,
          harness: harness ?? undefined,
          model: model && models.some((entry) => entry.id === model) ? model : undefined,
          text,
        }),
      );
      markStartedHere(sessionId);
      void connection.refresh();
      void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
      // Close the sheets, then the new session over the project it belongs to.
      router.dismissAll();
      router.push({
        pathname: "/project/[projectId]/session/[sessionId]",
        params: { projectId, sessionId },
      });
    } catch (reason) {
      setStarting(false);
      Alert.alert(
        "Couldn’t start the agent",
        reason instanceof Error ? reason.message : String(reason),
      );
    }
  };

  return (
    <View style={styles.root}>
      <Stack.Screen options={{ title: "Start agent", headerBackButtonDisplayMode: "minimal" }} />
      <SettingsPage>
        {item ? (
          <View style={styles.item}>
            <ProviderMark source={item.source} size={26} />
            <Text numberOfLines={2} style={styles.itemTitle}>
              {item.title}
            </Text>
          </View>
        ) : null}

        <View style={styles.field}>
          <FieldRow
            label="Project"
            open={open === "project"}
            onPress={() => setOpen(open === "project" ? null : "project")}
            value={
              project ? (
                <View style={styles.value}>
                  <ProjectMascot project={project.id} name={project.mascot} color={projectColor(project.id)} size={22} />
                  <View style={styles.valueText}>
                    <Text numberOfLines={1} style={styles.valueMain}>{project.name}</Text>
                    <Text numberOfLines={1} style={styles.valueSub}>
                      {project.id === item?.projectId ? `Matched from ${item.repo}` : "Not the item’s repo"}
                    </Text>
                  </View>
                </View>
              ) : (
                <Text style={styles.valueMain}>Choose a project</Text>
              )
            }
          />
          {open === "project"
            ? allProjects.map((entry) => (
                <Option
                  key={entry.id}
                  selected={entry.id === projectId}
                  onPress={() => {
                    setChosenProject(entry.id);
                    setHarness(undefined);
                    setModel(undefined);
                    setOpen(null);
                  }}
                >
                  <ProjectMascot project={entry.id} name={entry.mascot} color={projectColor(entry.id)} size={20} />
                  <Text style={styles.optionLabel}>{entry.name}</Text>
                </Option>
              ))
            : null}
          <FieldRow
            label="Agent"
            open={open === "agent"}
            onPress={() => setOpen(open === "agent" ? null : "agent")}
            value={
              <View style={styles.value}>
                {currentHarness ? <ProviderLogo harness={currentHarness} size={20} /> : null}
                <Text style={styles.valueMain}>
                  {catalog?.harnesses.find((entry) => entry.id === currentHarness)?.label ?? "Default"}
                </Text>
              </View>
            }
          />
          {open === "agent"
            ? (catalog?.harnesses ?? []).map((entry) => (
                <Option
                  key={entry.id}
                  selected={entry.id === currentHarness}
                  onPress={() => {
                    setHarness(entry.id);
                    setModel(undefined);
                    setOpen(null);
                  }}
                >
                  <ProviderLogo harness={entry.id} size={20} />
                  <Text style={styles.optionLabel}>{entry.label}</Text>
                </Option>
              ))
            : null}
          <FieldRow
            label="Model"
            open={open === "model"}
            onPress={() => setOpen(open === "model" ? null : "model")}
            value={<Text style={styles.valueMain}>{modelName}</Text>}
            disabled={models.length === 0}
          />
          {open === "model"
            ? models.map((entry) => (
                <Option
                  key={entry.id}
                  selected={entry.id === currentModel}
                  onPress={() => {
                    setModel(entry.id);
                    setOpen(null);
                  }}
                >
                  <Text style={styles.optionLabel}>{entry.name}</Text>
                </Option>
              ))
            : null}
        </View>

        <Text style={styles.heading}>WHAT SHOULD IT DO</Text>
        <View style={styles.modes}>
          {(
            [
              { id: "work", title: "Work on it", hint: "Make the change" },
              { id: "ask", title: "Just answer", hint: "Read and explain only" },
            ] as const
          ).map((entry) => (
            <Pressable
              key={entry.id}
              accessibilityRole="radio"
              accessibilityState={{ selected: mode === entry.id }}
              onPress={() => {
                void Haptics.selectionAsync();
                setMode(entry.id);
              }}
              style={[
                styles.mode,
                mode === entry.id && { borderColor: colors.accent, backgroundColor: withAlpha(colors.accent, 0.12) },
              ]}
            >
              <Text style={styles.modeTitle}>{entry.title}</Text>
              <Text style={styles.modeHint}>{entry.hint}</Text>
            </Pressable>
          ))}
        </View>

        <Text style={styles.heading}>EXTRA INSTRUCTIONS</Text>
        <TextInput
          value={text}
          onChangeText={setText}
          multiline
          placeholder="Optional. The agent already gets the item, its discussion and the diff."
          placeholderTextColor={colors.muted}
          keyboardAppearance={scheme}
          selectionColor={colors.accent}
          style={styles.input}
        />
        <View style={{ height: 96 + bottom }} />
      </SettingsPage>
      <View style={[styles.bar, { paddingBottom: bottom + 12 }]}>
        {!reachable ? (
          <Text style={styles.offline}>
            Your desktop is unreachable, so agents can’t start.
          </Text>
        ) : null}
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Start session"
          disabled={!item || !projectId || starting || !reachable}
          onPress={() => void start()}
          style={({ pressed }) => [
            styles.primary,
            (!item || !projectId || !reachable) && styles.disabled,
            pressed && styles.pressed,
          ]}
        >
          {starting ? (
            <ActivityIndicator color={colors.inverse} />
          ) : (
            <Icon name="sparkle" size={19} color={colors.inverse} />
          )}
          <Text style={styles.primaryLabel}>{starting ? "Starting…" : "Start session"}</Text>
        </Pressable>
      </View>
    </View>
  );
}

function FieldRow({
  label,
  value,
  open,
  onPress,
  disabled,
}: {
  label: string;
  value: ReactNode;
  open: boolean;
  onPress: () => void;
  disabled?: boolean;
}) {
  const styles = useStyles();
  const colors = useColors();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ expanded: open, disabled }}
      disabled={disabled}
      onPress={() => {
        void Haptics.selectionAsync();
        onPress();
      }}
      style={[styles.fieldRow, styles.divided]}
    >
      <Text style={styles.key}>{label}</Text>
      <View style={styles.fieldValue}>{value}</View>
      <View style={{ transform: [{ rotate: open ? "180deg" : "0deg" }] }}>
        <Icon name="chevronDown" size={16} color={disabled ? colors.faint : colors.muted} />
      </View>
    </Pressable>
  );
}

function Option({
  selected,
  onPress,
  children,
}: {
  selected: boolean;
  onPress: () => void;
  children: ReactNode;
}) {
  const styles = useStyles();
  const colors = useColors();
  return (
    <Pressable
      accessibilityRole="radio"
      accessibilityState={{ selected }}
      onPress={onPress}
      style={[styles.option, styles.divided]}
    >
      {children}
      {selected ? <Icon name="check" size={18} color={colors.accent} /> : null}
    </Pressable>
  );
}

const useStyles = themedStyles((colors) => ({
  root: {
    flex: 1,
  },
  item: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    marginBottom: 16,
  },
  itemTitle: {
    flex: 1,
    color: colors.content,
    fontSize: 17,
    fontWeight: "600",
  },
  field: {
    borderRadius: 16,
    borderCurve: "continuous",
    overflow: "hidden",
    backgroundColor: colors.listCard,
    borderWidth: 1,
    borderColor: colors.stroke,
  },
  fieldRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
    minHeight: 54,
    paddingVertical: 10,
    paddingHorizontal: 14,
  },
  divided: {
    borderTopWidth: 1,
    borderTopColor: colors.listDivider,
  },
  key: {
    width: 62,
    color: colors.muted,
    fontSize: 13,
  },
  fieldValue: {
    flex: 1,
    minWidth: 0,
  },
  value: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
  },
  valueText: {
    flex: 1,
    minWidth: 0,
  },
  valueMain: {
    color: colors.content,
    fontSize: 16,
    fontWeight: "600",
  },
  valueSub: {
    color: colors.muted,
    fontSize: 12,
  },
  option: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
    height: 46,
    paddingLeft: 88,
    paddingRight: 14,
  },
  optionLabel: {
    flex: 1,
    color: colors.content,
    fontSize: 15,
  },
  heading: {
    marginTop: 22,
    marginBottom: 8,
    color: colors.muted,
    fontSize: 13,
    fontWeight: "600",
  },
  modes: {
    flexDirection: "row",
    gap: 8,
  },
  mode: {
    flex: 1,
    gap: 3,
    padding: 12,
    borderRadius: 14,
    borderCurve: "continuous",
    borderWidth: 1.5,
    borderColor: colors.stroke,
    backgroundColor: colors.listCard,
  },
  modeTitle: {
    color: colors.content,
    fontSize: 15,
    fontWeight: "600",
  },
  modeHint: {
    color: colors.muted,
    fontSize: 12,
  },
  input: {
    minHeight: 110,
    padding: 14,
    paddingTop: 12,
    borderRadius: 16,
    borderCurve: "continuous",
    backgroundColor: colors.listCard,
    borderWidth: 1,
    borderColor: colors.stroke,
    color: colors.content,
    fontSize: 15,
    lineHeight: 21,
    textAlignVertical: "top",
  },
  bar: {
    position: "absolute",
    left: 0,
    right: 0,
    bottom: 0,
    gap: 8,
    paddingTop: 12,
    paddingHorizontal: 16,
    backgroundColor: withAlpha(colors.sheet, 0.94),
  },
  offline: {
    color: colors.muted,
    fontSize: 12.5,
    textAlign: "center",
  },
  primary: {
    height: 50,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
    gap: 8,
    borderRadius: 25,
    backgroundColor: colors.inverseSurface,
  },
  primaryLabel: {
    color: colors.inverse,
    fontSize: 17,
    fontWeight: "600",
  },
  disabled: {
    opacity: 0.35,
  },
  pressed: {
    opacity: 0.7,
  },
}));

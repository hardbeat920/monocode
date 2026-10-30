import { BottomSheet, RNHostView } from "@expo/ui";
import { useRef, useState } from "react";
import {
  Pressable,
  Text,
  TextInput,
  useWindowDimensions,
  View,
} from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { Icon } from "@/ui/Icon";
import { themedStyles, useColors } from "@/ui/theme";

import { type NewAction, useTerminalActions } from "./terminalActions";

/**
 * The commands a project's terminal menu offers, in a native sheet the height
 * of the list. They are the desktop footer's actions: a change here is sent
 * to the desktop, and one made there shows here. Each runs in the project's
 * folder when picked from the session's terminal button.
 */
export function TerminalActionsSheet({
  projectId,
  projectName,
  isPresented,
  onDismiss,
}: {
  projectId: string;
  projectName: string;
  isPresented: boolean;
  onDismiss: () => void;
}) {
  const colors = useColors();
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
        <View style={{ width, paddingBottom: Math.max(bottom, 16) + 8 }}>
          {/* Read fresh each time it opens, after the menu may have changed them. */}
          {isPresented ? (
            <ActionsEditor projectId={projectId} projectName={projectName} />
          ) : null}
        </View>
      </RNHostView>
    </BottomSheet>
  );
}

// No ScrollView: the sheet is as tall as the list, and a scroll view inside a
// hosted view has crashed Fabric as the page mounts (see CommitSheet).
function ActionsEditor({
  projectId,
  projectName,
}: {
  projectId: string;
  projectName: string;
}) {
  const [actions, save] = useTerminalActions(projectId);
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [error, setError] = useState<string | null>(null);
  const styles = useStyles();
  const colors = useColors();
  const commandInput = useRef<TextInput>(null);
  const ready = name.trim() !== "" && command.trim() !== "";
  const change = (next: NewAction[]) => {
    setError(null);
    save(next).catch((reason: unknown) =>
      setError(reason instanceof Error ? reason.message : String(reason)),
    );
  };
  const add = () => {
    if (!ready) return;
    change([
      ...actions,
      { name: name.trim(), command: command.trim(), icon: "play" },
    ]);
    setName("");
    setCommand("");
  };
  return (
    <View style={styles.content}>
      <View>
        <Text accessibilityRole="header" style={styles.title}>
          Actions
        </Text>
        <Text style={styles.project} numberOfLines={1}>
          {projectName} · the same as the desktop’s footer
        </Text>
      </View>
      <View style={styles.list}>
        {actions.map((action, index) => (
          <View
            key={action.id ?? `new-${index}`}
            style={[styles.row, index > 0 && styles.divider]}
          >
            <View style={styles.rowText}>
              <Text style={styles.name} numberOfLines={1}>
                {action.name}
              </Text>
              <Text style={styles.command} numberOfLines={1}>
                {action.command}
              </Text>
            </View>
            <Pressable
              accessibilityRole="button"
              accessibilityLabel={`Remove ${action.name}`}
              hitSlop={10}
              onPress={() => change(actions.filter((_, at) => at !== index))}
            >
              <Icon name="trash" size={18} color={colors.muted} />
            </Pressable>
          </View>
        ))}
        {actions.length === 0 ? (
          <Text style={styles.empty}>
            No actions yet. Add one here or from the desktop.
          </Text>
        ) : null}
      </View>
      <View style={styles.form}>
        <TextInput
          style={styles.input}
          value={name}
          onChangeText={setName}
          onSubmitEditing={() => commandInput.current?.focus()}
          placeholder="Name, like Run Dev"
          placeholderTextColor={colors.muted}
          returnKeyType="next"
          submitBehavior="submit"
          accessibilityLabel="New action's name"
        />
        <View style={styles.add}>
          <TextInput
            ref={commandInput}
            style={[styles.input, styles.mono, styles.grow]}
            value={command}
            onChangeText={setCommand}
            onSubmitEditing={add}
            placeholder="npm run dev"
            placeholderTextColor={colors.muted}
            autoCapitalize="none"
            autoCorrect={false}
            returnKeyType="done"
            accessibilityLabel="New action's command"
          />
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Add action"
            disabled={!ready}
            onPress={add}
            style={[styles.addButton, !ready && styles.disabled]}
          >
            <Text style={styles.addLabel}>Add</Text>
          </Pressable>
        </View>
      </View>
      {error ? <Text style={styles.error}>{error}</Text> : null}
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  content: { paddingHorizontal: 20, paddingTop: 28, gap: 16 },
  title: { color: colors.content, fontSize: 20, fontWeight: "600" },
  project: { color: colors.muted, fontSize: 13, marginTop: 2 },
  list: { borderRadius: 14, backgroundColor: colors.fill, overflow: "hidden" },
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
    paddingHorizontal: 14,
    paddingVertical: 11,
  },
  rowText: { flex: 1, gap: 2 },
  divider: { borderTopWidth: 1, borderTopColor: colors.background },
  name: { color: colors.content, fontSize: 15, fontWeight: "500" },
  command: { color: colors.muted, fontSize: 13, fontFamily: "Menlo" },
  empty: { color: colors.muted, padding: 14, fontSize: 14 },
  form: { gap: 8 },
  add: { flexDirection: "row", gap: 8 },
  input: {
    height: 44,
    paddingHorizontal: 14,
    borderRadius: 12,
    backgroundColor: colors.fill,
    color: colors.content,
    fontSize: 15,
  },
  mono: { fontFamily: "Menlo" },
  grow: { flex: 1 },
  addButton: {
    height: 44,
    paddingHorizontal: 18,
    borderRadius: 12,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.content,
  },
  addLabel: { color: colors.background, fontSize: 15, fontWeight: "600" },
  disabled: { opacity: 0.4 },
  error: { color: colors.deletions, fontSize: 13 },
}));

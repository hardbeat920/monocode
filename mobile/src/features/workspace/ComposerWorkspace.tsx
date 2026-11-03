import { Pressable, Text, View } from "react-native";

import { Icon } from "@/ui/Icon";
import { themedStyles, useColors } from "@/ui/theme";

import type { Workspace } from "./WorkspaceMenu";

/** A new draft's working copy, sharing the header's choice. A fresh worktree
 * is only created when the first message is sent. */
export function ComposerWorkspace({
  value,
  onChange,
  disabled = false,
}: {
  value: Workspace;
  onChange: (value: Workspace) => void;
  disabled?: boolean;
}) {
  const styles = useStyles();
  const colors = useColors();
  const current = value.mode === "current";
  const label =
    value.mode === "existing"
      ? value.worktree.name
      : current
        ? "Current checkout"
        : "New worktree";
  const content = (
    <>
      <Icon
        name={current ? "folder" : "branch"}
        size={14}
        color={colors.muted}
      />
      <Text style={styles.label} numberOfLines={1} ellipsizeMode="middle">
        {label}
      </Text>
    </>
  );

  // An existing worktree picked in the header keeps its exact path.
  if (value.mode === "existing")
    return <View style={styles.line}>{content}</View>;

  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityHint={
        current
          ? "Start this session in a new worktree"
          : "Start this session in the current checkout"
      }
      disabled={disabled}
      hitSlop={6}
      onPress={() => onChange({ mode: current ? "worktree" : "current" })}
      style={({ pressed }) => [
        styles.line,
        styles.choice,
        (pressed || disabled) && styles.dimmed,
      ]}
    >
      {content}
      <Icon name="chevronUpDown" size={14} color={colors.muted} />
    </Pressable>
  );
}

const useStyles = themedStyles((colors) => ({
  line: {
    flexShrink: 1,
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  choice: {
    marginLeft: -6,
    height: 30,
    paddingHorizontal: 10,
    borderRadius: 15,
    backgroundColor: colors.fill,
  },
  label: {
    flexShrink: 1,
    color: colors.content,
    fontSize: 13,
    fontWeight: "500",
  },
  dimmed: {
    opacity: 0.6,
  },
}));

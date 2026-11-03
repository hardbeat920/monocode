import { Text, View } from "react-native";
import { formatCount, useDiffTheme } from "@/features/changes/diffTheme";
import { ChoiceMenu } from "@/ui/ChoiceMenu";
import { Icon } from "@/ui/Icon";
import { useColors } from "@/ui/theme";
import type { ChangedFile, Worktree } from "./types";
import type { Workspace } from "./WorkspaceMenu";
export type { Workspace } from "./WorkspaceMenu";

export function WorkspaceMenu({
  value,
  worktrees,
  onChange,
  branch,
  checkoutBranch,
  files,
  loading,
  onOpenChanges,
}: {
  value: Workspace;
  worktrees: Worktree[];
  onChange: (value: Workspace) => void;
  branch: string;
  checkoutBranch?: string;
  files: ChangedFile[];
  loading: boolean;
  onOpenChanges: () => void;
}) {
  return (
    <ChoiceMenu
      style={{ maxWidth: "100%", flexShrink: 1 }}
      label={`Workspace: ${branch}`}
      actions={[
        {
          label: `Current checkout${checkoutBranch ? ` (${checkoutBranch})` : ""}`,
          selected: value.mode === "current",
          onPress: () => onChange({ mode: "current" }),
        },
        {
          label: "New worktree",
          selected: value.mode === "worktree",
          onPress: () => onChange({ mode: "worktree" }),
        },
        ...worktrees.map((worktree) => ({
          label: `${worktree.name} — ${worktree.path}`,
          selected:
            value.mode === "existing" && value.worktree.path === worktree.path,
          onPress: () => onChange({ mode: "existing", worktree }),
        })),
        {
          label: "View changes",
          disabled: value.mode === "worktree",
          onPress: onOpenChanges,
        },
      ]}
    >
      <View style={{ padding: 8, maxWidth: "100%" }}>
        <WorkspaceLine
          current={value.mode === "current"}
          branch={branch}
          files={files}
          loading={loading}
        />
      </View>
    </ChoiceMenu>
  );
}
export function WorkspaceLine({
  current,
  branch,
  files,
  loading = false,
}: {
  current: boolean;
  branch: string;
  files: ChangedFile[];
  loading?: boolean;
}) {
  const colors = useColors();
  const diff = useDiffTheme();
  return (
    <View
      style={{
        flexDirection: "row",
        alignItems: "center",
        gap: 6,
        flexShrink: 1,
        maxWidth: "100%",
        minWidth: 0,
      }}
    >
      <Icon
        name={current ? "folder" : "branch"}
        size={14}
        color={colors.muted}
      />
      <Text
        numberOfLines={1}
        ellipsizeMode="middle"
        accessibilityLabel={branch}
        style={{
          color: colors.content,
          flexShrink: 1,
          minWidth: 0,
          maxWidth: 160,
        }}
      >
        {branch}
      </Text>
      <Text numberOfLines={1} style={{ color: colors.muted, flexShrink: 0 }}>
        {loading
          ? "· Loading changes…"
          : `· ${formatCount(files.length)} files`}
      </Text>
      {!loading ? (
        <>
          <Text numberOfLines={1} style={{ color: diff.add.fg, flexShrink: 0 }}>
            +{formatCount(files.reduce((n, f) => n + f.additions, 0))}
          </Text>
          <Text numberOfLines={1} style={{ color: diff.del.fg, flexShrink: 0 }}>
            -{formatCount(files.reduce((n, f) => n + f.deletions, 0))}
          </Text>
        </>
      ) : null}
    </View>
  );
}

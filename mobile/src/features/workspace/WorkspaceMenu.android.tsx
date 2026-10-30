import { useState, type ReactElement, type ReactNode } from "react";
import {
  ActivityIndicator,
  Modal,
  Pressable,
  ScrollView,
  Text,
  View,
  useWindowDimensions,
} from "react-native";
import Svg, { Path } from "react-native-svg";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { formatCount, useDiffTheme } from "@/features/changes/diffTheme";
import { Icon, type IconName } from "@/ui/Icon";
import { themedStyles, useColors } from "@/ui/theme";
import type { ChangedFile, Worktree } from "./types";
import type { Workspace } from "./WorkspaceMenu";
export type { Workspace } from "./WorkspaceMenu";

const keyOf = (value: Workspace) =>
  value.mode === "existing" ? value.worktree.path : value.mode;

/** Android counterpart of the grouped iOS Workspace sheet and its Worktrees page. */
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
  const styles = useStyles();
  const colors = useColors();
  const diff = useDiffTheme();
  const insets = useSafeAreaInsets();
  const { height } = useWindowDimensions();
  const [open, setOpen] = useState(false);
  const [existingPage, setExistingPage] = useState(false);
  const [pending, setPending] = useState<string | null>(null);
  const existing = value.mode === "existing" ? value.worktree : null;
  const dismiss = () => {
    setOpen(false);
    setExistingPage(false);
    setPending(null);
  };
  // As on iOS, close only once the picked working copy has been read.
  if (pending && !loading) {
    setPending(null);
    setOpen(false);
    setExistingPage(false);
  }
  const pick = (next: Workspace) => {
    if (pending) return;
    if (keyOf(next) === keyOf(value)) return dismiss();
    onChange(next);
    setPending(keyOf(next));
  };
  const additions = files.reduce((sum, file) => sum + file.additions, 0);
  const deletions = files.reduce((sum, file) => sum + file.deletions, 0);
  return (
    <>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={`Workspace: ${branch}`}
        style={styles.trigger}
        onPress={() => {
          setExistingPage(false);
          setOpen(true);
        }}
      >
        <WorkspaceLine
          current={value.mode === "current"}
          branch={branch}
          files={files}
          loading={loading}
        />
        <Icon name="chevronDown" size={12} color={colors.muted} />
      </Pressable>
      <Modal
        visible={open}
        transparent
        animationType="none"
        statusBarTranslucent
        onRequestClose={() =>
          existingPage ? setExistingPage(false) : dismiss()
        }
      >
        <View style={styles.overlay}>
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Close workspace"
            style={styles.backdrop}
            onPress={dismiss}
          />
          <View
            accessibilityViewIsModal
            style={[
              styles.sheet,
              {
                height: Math.min(height * 0.58, 480),
                maxHeight: height - insets.top - 16,
                paddingBottom: insets.bottom + 12,
              },
            ]}
          >
            <View style={styles.handle} />
            <View style={styles.header}>
              {existingPage ? (
                <Pressable
                  accessibilityRole="button"
                  accessibilityLabel="Back to Workspace"
                  style={styles.back}
                  onPress={() => setExistingPage(false)}
                >
                  <View style={{ transform: [{ rotate: "180deg" }] }}>
                    <Icon name="chevronRight" size={24} />
                  </View>
                </Pressable>
              ) : null}
              <Text accessibilityRole="header" style={styles.title}>
                {existingPage ? "Worktrees" : "Workspace"}
              </Text>
            </View>
            <ScrollView
              style={styles.scroll}
              contentContainerStyle={styles.content}
            >
              {existingPage ? (
                <View style={styles.card}>
                  {worktrees.map((tree, index) => (
                    <View key={tree.path}>
                      {index > 0 ? <View style={styles.divider} /> : null}
                      <WorkspaceRow
                        icon="branch"
                        title={tree.name}
                        hint={tree.path}
                        selected={existing?.path === tree.path}
                        busy={pending === tree.path}
                        disabled={!!pending}
                        onPress={() =>
                          pick({ mode: "existing", worktree: tree })
                        }
                      />
                    </View>
                  ))}
                </View>
              ) : (
                <>
                  <Text style={styles.heading}>Start the session in</Text>
                  <View style={styles.card}>
                    <WorkspaceRow
                      icon="folder"
                      title="Current checkout"
                      hint={checkoutBranch || "The project's own folder"}
                      selected={value.mode === "current"}
                      busy={pending === "current"}
                      disabled={!!pending}
                      onPress={() => pick({ mode: "current" })}
                    />
                    <View style={styles.divider} />
                    <WorkspaceRow
                      icon="branch"
                      title="New worktree"
                      hint="A fresh branch, in a folder of its own"
                      selected={value.mode === "worktree"}
                      busy={pending === "worktree"}
                      disabled={!!pending}
                      onPress={() => pick({ mode: "worktree" })}
                    />
                    {worktrees.length ? (
                      <>
                        <View style={styles.divider} />
                        <WorkspaceRow
                          icon="folder"
                          title="Existing worktree"
                          hint={
                            existing?.name ||
                            `${worktrees.length} ${worktrees.length === 1 ? "worktree" : "worktrees"}`
                          }
                          selected={!!existing}
                          chevron
                          disabled={!!pending}
                          onPress={() => setExistingPage(true)}
                        />
                      </>
                    ) : null}
                  </View>
                  <View style={[styles.card, styles.changes]}>
                    <WorkspaceRow
                      icon={
                        <Svg width={22} height={22} viewBox="0 0 24 24">
                          <Path
                            d="M3 7h8M7 3v8M5 21 19 3M15 17h6"
                            stroke={colors.content}
                            strokeWidth={1.8}
                            strokeLinecap="round"
                          />
                        </Svg>
                      }
                      title="View changes"
                      hint={
                        loading
                          ? "Loading changes…"
                          : `${formatCount(files.length)} ${files.length === 1 ? "file" : "files"} changed`
                      }
                      disabled={value.mode === "worktree" || !!pending}
                      onPress={() => {
                        dismiss();
                        // The unanimated Android modal is removed before presenting Changes.
                        requestAnimationFrame(onOpenChanges);
                      }}
                      accessory={
                        loading ? (
                          <ActivityIndicator color={colors.muted} />
                        ) : (
                          <View style={styles.totals}>
                            <Text style={{ color: diff.add.fg }}>
                              +{formatCount(additions)}
                            </Text>
                            <Text style={{ color: diff.del.fg }}>
                              -{formatCount(deletions)}
                            </Text>
                          </View>
                        )
                      }
                    />
                  </View>
                  {value.mode === "worktree" ? (
                    <Text style={styles.note}>
                      Changes are available after the worktree is created.
                    </Text>
                  ) : null}
                </>
              )}
            </ScrollView>
          </View>
        </View>
      </Modal>
    </>
  );
}

function WorkspaceRow({
  icon,
  title,
  hint,
  selected,
  busy,
  chevron,
  disabled,
  onPress,
  accessory,
}: {
  icon: IconName | ReactElement;
  title: string;
  hint: string;
  selected?: boolean;
  busy?: boolean;
  chevron?: boolean;
  disabled?: boolean;
  onPress: () => void;
  accessory?: ReactNode;
}) {
  const styles = useStyles();
  const colors = useColors();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={title}
      accessibilityState={{ selected, disabled }}
      disabled={disabled}
      onPress={onPress}
      style={[styles.row, disabled && styles.disabled]}
    >
      {typeof icon === "string" ? (
        <Icon name={icon} size={22} color={colors.content} />
      ) : (
        icon
      )}
      <View style={styles.labels}>
        <Text numberOfLines={1} ellipsizeMode="middle" style={styles.rowTitle}>
          {title}
        </Text>
        <Text numberOfLines={1} ellipsizeMode="middle" style={styles.hint}>
          {hint}
        </Text>
      </View>
      {accessory ||
        (busy ? (
          <ActivityIndicator color={colors.muted} />
        ) : selected ? (
          <Icon name="check" size={20} color={colors.accent} />
        ) : chevron ? (
          <Icon name="chevronRight" size={20} color={colors.muted} />
        ) : null)}
    </Pressable>
  );
}

const useStyles = themedStyles((colors) => ({
  trigger: {
    padding: 8,
    maxWidth: "100%",
    flexShrink: 1,
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  overlay: {
    flex: 1,
    justifyContent: "flex-end",
    backgroundColor: "rgba(0,0,0,0.4)",
  },
  backdrop: { flex: 1 },
  sheet: {
    backgroundColor: colors.sheet,
    borderTopLeftRadius: 32,
    borderTopRightRadius: 32,
    overflow: "hidden",
  },
  handle: {
    width: 40,
    height: 4,
    borderRadius: 2,
    backgroundColor: colors.faint,
    alignSelf: "center",
    marginTop: 8,
  },
  header: { height: 56, alignItems: "center", justifyContent: "center" },
  title: { color: colors.content, fontSize: 20, fontWeight: "600" },
  back: {
    position: "absolute",
    left: 12,
    width: 44,
    height: 44,
    borderRadius: 22,
    backgroundColor: colors.fill,
    alignItems: "center",
    justifyContent: "center",
  },
  scroll: { flex: 1 },
  content: { paddingHorizontal: 16, paddingTop: 12, paddingBottom: 12 },
  heading: {
    color: colors.muted,
    fontWeight: "600",
    fontSize: 15,
    marginHorizontal: 16,
    marginBottom: 12,
  },
  card: {
    borderRadius: 26,
    backgroundColor: colors.surfaceRaised,
    overflow: "hidden",
  },
  changes: { marginTop: 12 },
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: 14,
    minHeight: 72,
    paddingHorizontal: 16,
    paddingVertical: 12,
  },
  labels: { flex: 1, minWidth: 0, gap: 2 },
  rowTitle: { color: colors.content, fontSize: 17 },
  hint: { color: colors.muted, fontSize: 13 },
  divider: {
    height: 0.5,
    backgroundColor: colors.stroke,
    marginLeft: 52,
    marginRight: 16,
  },
  totals: { flexDirection: "row", gap: 6 },
  disabled: { opacity: 0.4 },
  note: {
    color: colors.muted,
    fontSize: 12,
    marginTop: 8,
    marginHorizontal: 16,
  },
}));

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

import * as Clipboard from "expo-clipboard";
import * as Haptics from "expo-haptics";
import { router, Stack } from "expo-router";
import { useState } from "react";
import {
  Alert,
  Modal,
  Platform,
  Pressable,
  Text,
  TextInput,
  useWindowDimensions,
  View,
} from "react-native";
import Svg, { Circle, Path } from "react-native-svg";

import type { SessionSummary } from "@/features/workspace/types";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { LiquidView } from "@/ui/LiquidView";
import { ChoiceMenu } from "@/ui/ChoiceMenu";
import { themedStyles, useColors } from "@/ui/theme";

import { useSessionActions } from "./useSessionActions";

const failed = (title: string) => (reason: unknown) =>
  Alert.alert(title, reason instanceof Error ? reason.message : String(reason));

/** What the back button and the buttons on the right take of the header's
 *  width, with a little gap before them; the changes count's badge overlaps
 *  its button and takes none. Too little left for them and iOS drops a
 *  button rather than squeeze them. */
const HEADER_ITEMS = 210;

/**
 * A session's title on the left of the stack's header, beside the back button
 * rather than centred: its title over its project and branch, each cut short
 * before the buttons on the right.
 */
export function SessionTitle({
  title,
  subtitle,
}: {
  title: string;
  subtitle: string;
}) {
  const styles = useStyles();
  const { width } = useWindowDimensions();
  if (Platform.OS === "android")
    return (
      <View style={styles.androidTitle}>
        <Text style={styles.name} numberOfLines={1} ellipsizeMode="tail">
          {title}
        </Text>
        <Text style={styles.meta} numberOfLines={1} ellipsizeMode="middle">
          {subtitle}
        </Text>
      </View>
    );
  return (
    <Stack.Toolbar placement="left">
      <Stack.Toolbar.View hidesSharedBackground>
        <View
          style={[styles.title, { width: Math.max(width - HEADER_ITEMS, 80) }]}
        >
          <Text style={styles.name} numberOfLines={1}>
            {title}
          </Text>
          <Text style={styles.meta} numberOfLines={1}>
            {subtitle}
          </Text>
        </View>
      </Stack.Toolbar.View>
    </Stack.Toolbar>
  );
}

/**
 * A session's buttons on the right of the stack's header: its changes, which
 * open in a sheet to stage and commit, counted on the button as the desktop's
 * Changes tab is; then a menu with the desktop sidebar's actions for it.
 */
export function SessionToolbar({
  projectId,
  sessionId,
  session,
  worktreePath,
  harness,
  changed,
  onSettings,
}: {
  projectId: string;
  sessionId: string;
  /** Unset until the session is in the catalog; its actions wait for it. */
  session: SessionSummary | undefined;
  worktreePath?: string;
  /** The composer's harness, which writes commit messages and pull requests. */
  harness?: string;
  /** Files changed in the session's working copy. */
  changed: number;
  onSettings: () => void;
}) {
  const { archiveSession, deleteSession, renameSession } = useWorkspace();
  const { rename } = useSessionActions(() => {});
  const styles = useStyles();
  const colors = useColors();
  const [renaming, setRenaming] = useState(false);
  const [name, setName] = useState("");

  const openChanges = () =>
    router.push({
      pathname: "/changes",
      params: {
        projectId,
        ...(worktreePath ? { worktreePath } : {}),
        ...(harness ? { harness } : {}),
      },
    });

  const copyId = async () => {
    await Clipboard.setStringAsync(sessionId);
    void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
  };

  // Back to the project first, so the session isn't left on screen as it goes.
  const leaveAnd = (
    remove: (id: string) => Promise<void>,
    title: string,
    target: SessionSummary,
  ) => {
    router.back();
    remove(target.id).catch(failed(title));
  };
  const archive = (target: SessionSummary) =>
    Alert.alert(
      `Archive “${target.title}”?`,
      "It moves to the archive on your desktop.",
      [
        { text: "Cancel", style: "cancel" },
        {
          text: "Archive",
          isPreferred: true,
          onPress: () =>
            leaveAnd(archiveSession, "Could not archive the session", target),
        },
      ],
    );
  const remove = (target: SessionSummary) =>
    Alert.alert(`Delete “${target.title}”?`, "This can’t be undone.", [
      { text: "Cancel", style: "cancel" },
      {
        text: "Delete",
        style: "destructive",
        onPress: () =>
          leaveAnd(deleteSession, "Could not delete the session", target),
      },
    ]);

  if (Platform.OS === "android")
    return (
      <View style={styles.androidToolbar}>
        <LiquidView borderRadius={24} innerStyle={styles.androidActions}>
          <Pressable
            accessibilityRole="button"
            accessibilityLabel={`Changes, ${changed} ${changed === 1 ? "file" : "files"}`}
            onPress={openChanges}
            style={styles.androidButton}
          >
            <Svg width={24} height={24} viewBox="0 0 24 24">
              <Path
                d="M3 7h8M7 3v8M5 21 19 3M15 17h6"
                stroke={colors.content}
                strokeWidth={1.8}
                strokeLinecap="round"
              />
            </Svg>
          </Pressable>
          <ChoiceMenu
            label="Session actions"
            actions={[
              {
                label: "Rename",
                disabled: !session,
                onPress: () => {
                  if (session) {
                    setName(session.title);
                    setRenaming(true);
                  }
                },
              },
              { label: "Copy Session ID", onPress: () => void copyId() },
              { label: "Settings", onPress: onSettings },
              {
                label: "Archive",
                disabled: !session,
                onPress: () => session && archive(session),
              },
              {
                label: "Delete",
                disabled: !session,
                onPress: () => session && remove(session),
              },
            ]}
          >
            <View style={styles.androidButton}>
              <Svg width={24} height={24} viewBox="0 0 24 24">
                {[4, 12, 20].map((cx) => (
                  <Circle
                    key={cx}
                    cx={cx}
                    cy={12}
                    r={2}
                    fill={colors.content}
                  />
                ))}
              </Svg>
            </View>
          </ChoiceMenu>
        </LiquidView>
        {/* Keep the overhang outside both the pill and the button's clip bounds. */}
        {changed > 0 ? (
          <View pointerEvents="none" style={styles.badge}>
            <Text style={styles.badgeText}>{changed}</Text>
          </View>
        ) : null}
        {/* Alert.prompt is iOS-only; keep Android's restored Rename action usable. */}
        <Modal
          visible={renaming}
          transparent
          animationType="fade"
          onRequestClose={() => setRenaming(false)}
        >
          <View style={styles.renameBackdrop}>
            <View style={styles.renameDialog}>
              <Text accessibilityRole="header" style={styles.name}>
                Rename session
              </Text>
              <TextInput
                accessibilityLabel="Session name"
                value={name}
                onChangeText={setName}
                autoFocus
                selectTextOnFocus
                style={styles.renameInput}
              />
              <View style={styles.renameActions}>
                <Pressable
                  accessibilityRole="button"
                  onPress={() => setRenaming(false)}
                  style={styles.dialogButton}
                >
                  <Text style={styles.name}>Cancel</Text>
                </Pressable>
                <Pressable
                  accessibilityRole="button"
                  disabled={!name.trim()}
                  style={styles.dialogButton}
                  onPress={() => {
                    setRenaming(false);
                    if (session && name.trim() && name.trim() !== session.title)
                      void renameSession(session, name.trim()).catch(
                        failed("Could not rename the session"),
                      );
                  }}
                >
                  <Text style={styles.name}>Rename</Text>
                </Pressable>
              </View>
            </View>
          </View>
        </Modal>
      </View>
    );

  return (
    <Stack.Toolbar placement="right">
      <Stack.Toolbar.Button
        icon="plus.forwardslash.minus"
        accessibilityLabel={`Changes, ${changed} ${changed === 1 ? "file" : "files"}`}
        onPress={openChanges}
      >
        {changed > 0 ? (
          <Stack.Toolbar.Badge>{String(changed)}</Stack.Toolbar.Badge>
        ) : null}
      </Stack.Toolbar.Button>
      <Stack.Toolbar.Menu icon="ellipsis" accessibilityLabel="Session actions">
        <Stack.Toolbar.MenuAction
          icon="pencil"
          disabled={!session}
          onPress={() => session && rename(session)}
        >
          Rename
        </Stack.Toolbar.MenuAction>
        <Stack.Toolbar.MenuAction
          icon="doc.on.doc"
          onPress={() => void copyId()}
        >
          Copy Session ID
        </Stack.Toolbar.MenuAction>
        <Stack.Toolbar.MenuAction icon="gearshape" onPress={onSettings}>
          Settings
        </Stack.Toolbar.MenuAction>
        <Stack.Toolbar.Menu inline>
          <Stack.Toolbar.MenuAction
            icon="archivebox"
            disabled={!session}
            onPress={() => session && archive(session)}
          >
            Archive
          </Stack.Toolbar.MenuAction>
          <Stack.Toolbar.MenuAction
            icon="trash"
            destructive
            disabled={!session}
            onPress={() => session && remove(session)}
          >
            Delete
          </Stack.Toolbar.MenuAction>
        </Stack.Toolbar.Menu>
      </Stack.Toolbar.Menu>
    </Stack.Toolbar>
  );
}

const useStyles = themedStyles((colors) => ({
  androidTitle: {
    flexShrink: 1,
    minWidth: 0,
    maxWidth: "100%",
  },
  // Reserve the badge's overhang inside the native header's measured bounds.
  androidToolbar: { paddingTop: 6 },
  androidActions: {
    flexDirection: "row",
    alignItems: "center",
    flexShrink: 0,
    borderRadius: 24,
  },
  androidButton: {
    width: 40,
    height: 44,
    alignItems: "center",
    justifyContent: "center",
  },
  badge: {
    position: "absolute",
    top: 0,
    right: 40,
    minWidth: 20,
    height: 20,
    paddingHorizontal: 4,
    borderRadius: 10,
    backgroundColor: "#ff3b30",
    alignItems: "center",
    justifyContent: "center",
  },
  badgeText: { color: "#ffffff", fontSize: 12, fontWeight: "600" },
  renameBackdrop: {
    flex: 1,
    justifyContent: "center",
    padding: 24,
    backgroundColor: "rgba(0,0,0,0.4)",
  },
  renameDialog: {
    borderRadius: 20,
    padding: 20,
    gap: 16,
    backgroundColor: colors.sheet,
  },
  renameInput: {
    color: colors.content,
    backgroundColor: colors.fill,
    borderRadius: 8,
    padding: 12,
    fontSize: 16,
  },
  renameActions: { flexDirection: "row", justifyContent: "flex-end", gap: 8 },
  dialogButton: { padding: 12 },
  // Pulled back over the gap iOS leaves between bar items, to sit close to
  // the back button.
  title: {
    marginLeft: -8,
  },
  name: {
    color: colors.content,
    fontSize: 17,
    fontWeight: "700",
  },
  meta: {
    color: colors.muted,
    fontSize: 13,
  },
}));

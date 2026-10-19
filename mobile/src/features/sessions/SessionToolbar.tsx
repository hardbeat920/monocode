import * as Clipboard from "expo-clipboard";
import * as Haptics from "expo-haptics";
import { router, Stack } from "expo-router";
import { Alert, Text, useWindowDimensions, View } from "react-native";

import type { SessionSummary } from "@/features/workspace/types";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { themedStyles } from "@/ui/theme";

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
export function SessionTitle({ title, subtitle }: { title: string; subtitle: string }) {
  const styles = useStyles();
  const { width } = useWindowDimensions();
  return (
    <Stack.Toolbar placement="left">
      <Stack.Toolbar.View hidesSharedBackground>
        <View style={[styles.title, { width: Math.max(width - HEADER_ITEMS, 80) }]}>
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
  const { archiveSession, deleteSession } = useWorkspace();
  const { rename } = useSessionActions(() => {});

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

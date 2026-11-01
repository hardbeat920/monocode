import * as Haptics from "expo-haptics";
import { useEffect, useRef, useState } from "react";
import { ActionSheetIOS, Alert } from "react-native";
import type { SwipeableMethods } from "react-native-gesture-handler/ReanimatedSwipeable";

import type { SessionSummary } from "@/features/workspace/types";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { useScheme } from "@/ui/theme";

import {
  archiveMany,
  archiveWithUndo,
  confirmArchive,
  confirmArchiveMany,
  type ToastSink,
} from "./archiveActions";

/** How long a deleted row takes to leave and the rows below to close up. */
export const REMOVE_MS = 240;

const failed = (title: string) => (reason: unknown) =>
  Alert.alert(title, reason instanceof Error ? reason.message : String(reason));

/**
 * What the desktop sidebar's menu does for a session, from a swipe or a long
 * press on a row of a session list: rename and delete, each asked first, and
 * archive where a `toast` says what it did. Archive takes the session off the
 * list at once, with Undo where the desktop can unarchive; without Undo it is
 * asked first, as the session's own menu does. Also keeps one row's swipe
 * actions out at a time.
 */
export function useSessionActions(
  onDeleted: (session: SessionSummary) => void,
  toast?: ToastSink,
) {
  const scheme = useScheme();
  const { renameSession, deleteSession, archiveSession, unarchiveSession, canUnarchive } =
    useWorkspace();

  const openRow = useRef<SwipeableMethods | null>(null);
  const swipeOpened = (row: SwipeableMethods) => {
    if (openRow.current !== row) openRow.current?.close();
    openRow.current = row;
  };
  const closeOpenRow = () => {
    openRow.current?.close();
    openRow.current = null;
  };

  // A deleted row fades out and the rows below slide up into its place. The
  // animations are on only while it goes, so switching projects, searching and
  // paging stay instant; the delete waits a render for them to be in place.
  const [removal, setRemoval] = useState<{
    session: SessionSummary;
    kind: "delete" | "archive";
  } | null>(null);
  const removing = removal?.session ?? null;
  useEffect(() => {
    if (!removal) return;
    const { session, kind } = removal;
    const done =
      kind === "archive" && toast
        ? archiveWithUndo(
            session,
            {
              archive: archiveSession,
              unarchive: canUnarchive ? unarchiveSession : undefined,
            },
            toast,
          )
        : deleteSession(session.id).then(
            () => onDeleted(session),
            failed("Could not delete the session"),
          );
    void done.finally(() => setTimeout(() => setRemoval(null), REMOVE_MS));
    // Runs once per removal, with the rest as it was when it was asked for.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [removal]);

  const rename = (session: SessionSummary) =>
    Alert.prompt(
      "Rename session",
      undefined,
      [
        { text: "Cancel", style: "cancel" },
        {
          text: "Rename",
          isPreferred: true,
          onPress: (title?: string) => {
            const trimmed = title?.trim();
            if (trimmed && trimmed !== session.title)
              renameSession(session, trimmed).catch(
                failed("Could not rename the session"),
              );
          },
        },
      ],
      "plain-text",
      session.title,
    );
  const remove = (session: SessionSummary) =>
    Alert.alert(`Delete “${session.title}”?`, "This can’t be undone.", [
      { text: "Cancel", style: "cancel" },
      {
        text: "Delete",
        style: "destructive",
        onPress: () => setRemoval({ session, kind: "delete" }),
      },
    ]);
  const archive = (session: SessionSummary) => {
    if (!toast) return;
    if (canUnarchive) setRemoval({ session, kind: "archive" });
    else confirmArchive(session, () => setRemoval({ session, kind: "archive" }));
  };
  /** Archives each of `sessions`, all from one Mono, once asked. */
  const archiveFinished = (sessions: SessionSummary[], from: string) => {
    if (!toast || !sessions.length) return;
    confirmArchiveMany(sessions.length, from, () => {
      void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
      void archiveMany(sessions, { archive: archiveSession }, toast);
    });
  };
  const showActions = (session: SessionSummary) => {
    void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Medium);
    ActionSheetIOS.showActionSheetWithOptions(
      {
        title: session.title,
        options: toast ? ["Rename", "Archive", "Delete", "Cancel"] : ["Rename", "Delete", "Cancel"],
        destructiveButtonIndex: toast ? 2 : 1,
        cancelButtonIndex: toast ? 3 : 2,
        userInterfaceStyle: scheme,
      },
      (index) => {
        if (index === 0) rename(session);
        else if (toast && index === 1) archive(session);
        else if (index === (toast ? 2 : 1)) remove(session);
      },
    );
  };

  return {
    removing,
    rename,
    remove,
    archive,
    archiveFinished,
    showActions,
    swipeOpened,
    closeOpenRow,
  };
}

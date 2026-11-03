import * as Haptics from "expo-haptics";
import { useEffect, useRef, useState } from "react";
import { ActionSheetIOS, Alert } from "react-native";
import type { SwipeableMethods } from "react-native-gesture-handler/ReanimatedSwipeable";

import type { SessionSummary } from "@/features/workspace/types";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { useScheme } from "@/ui/theme";

/** How long a deleted row takes to leave and the rows below to close up. */
export const REMOVE_MS = 240;

const failed = (title: string) => (reason: unknown) =>
  Alert.alert(title, reason instanceof Error ? reason.message : String(reason));

/**
 * What the desktop sidebar's menu does for a session, from a swipe or a long
 * press on a row of a session list: rename and delete, each asked first. Also
 * keeps one row's swipe actions out at a time.
 */
export function useSessionActions(
  onDeleted: (session: SessionSummary) => void,
) {
  const scheme = useScheme();
  const { renameSession, deleteSession } = useWorkspace();

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
  const [removing, setRemoving] = useState<SessionSummary | null>(null);
  useEffect(() => {
    if (!removing) return;
    deleteSession(removing.id)
      .then(() => onDeleted(removing), failed("Could not delete the session"))
      .finally(() => setTimeout(() => setRemoving(null), REMOVE_MS));
    // Runs once per delete, with the rest as it was when it was asked for.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [removing]);

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
        onPress: () => setRemoving(session),
      },
    ]);
  const showActions = (session: SessionSummary) => {
    void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Medium);
    ActionSheetIOS.showActionSheetWithOptions(
      {
        title: session.title,
        options: ["Rename", "Delete", "Cancel"],
        destructiveButtonIndex: 1,
        cancelButtonIndex: 2,
        userInterfaceStyle: scheme,
      },
      (index) => {
        if (index === 0) rename(session);
        else if (index === 1) remove(session);
      },
    );
  };

  return { removing, rename, remove, showActions, swipeOpened, closeOpenRow };
}

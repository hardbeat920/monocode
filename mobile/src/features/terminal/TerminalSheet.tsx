import { BottomSheet, RNHostView } from "@expo/ui";
import {
  frame,
  ignoreSafeArea,
  interactiveDismissDisabled,
} from "@expo/ui/swift-ui/modifiers";
import { useState } from "react";
import { Platform, View } from "react-native";

import type { Project } from "@/features/workspace/types";
import { themedStyles, useColors } from "@/ui/theme";

import { TerminalPane } from "./TerminalScreen";

/**
 * The terminal in a full-height native sheet over the session. Swiping down
 * puts it away, from the terminal once its history is scrolled to the top.
 * `opened` changes each time it is asked for, so an action picked while it is
 * up runs too.
 */
export function TerminalSheet({
  project,
  worktreePath,
  run,
  opened,
  isPresented,
  onDismiss,
}: {
  project: Project;
  worktreePath?: string;
  run?: string;
  opened: number;
  isPresented: boolean;
  onDismiss: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  // The pane says when a drag may swipe it away: the terminal's history is a
  // native scroll area, so iOS scrolls it and swipes the sheet only from its
  // top, as any sheet with a list does.
  const [swipeAway, setSwipeAway] = useState(true);
  return (
    <BottomSheet
      isPresented={isPresented}
      onDismiss={onDismiss}
      snapPoints={["full"]}
      contentPadding={0}
      containerColor={colors.background}
      // SwiftUI's: fill the sheet, and swipe away when the pane allows. The
      // pane keeps above the keyboard itself, frame by frame, as the composer
      // does; SwiftUI would resize it once, in a jump.
      modifiers={
        Platform.OS === "ios"
          ? [
              frame({
                maxWidth: Infinity,
                maxHeight: Infinity,
                alignment: "topLeading",
              }),
              ignoreSafeArea({ regions: "keyboard", edges: "bottom" }),
              interactiveDismissDisabled(!swipeAway),
            ]
          : undefined
      }
    >
      <RNHostView>
        <View style={styles.fill}>
          {isPresented ? (
            <TerminalPane
              key={opened}
              project={project}
              worktreePath={worktreePath}
              run={run}
              chrome="sheet"
              onLeave={onDismiss}
              onSwipeAway={setSwipeAway}
            />
          ) : null}
        </View>
      </RNHostView>
    </BottomSheet>
  );
}

const useStyles = themedStyles(() => ({
  fill: { flex: 1 },
}));

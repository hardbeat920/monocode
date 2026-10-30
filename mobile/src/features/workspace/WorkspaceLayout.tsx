import { useState, type ReactNode } from "react";
import { StyleSheet, View } from "react-native";
import { SafeAreaProvider, useSafeAreaInsets } from "react-native-safe-area-context";

import { ContentPane } from "@/ui/ContentPane";
import { useColors } from "@/ui/theme";

import { WorkspaceSidebar } from "./WorkspaceSidebar";

// Leave a usable chat beside the sidebar, including the native header's actions.
const SIDEBAR_WIDTH = 260;
const MIN_CONTENT_WIDTH = 480;

/** One navigator for both layouts. Resizing changes only its available space;
 * the connection, route, composer and transcript all keep their identities. */
export function WorkspaceLayout({ children }: { children: ReactNode }) {
  const colors = useColors();
  const insets = useSafeAreaInsets();
  const [width, setWidth] = useState(0);
  const expanded = width - insets.left - insets.right >= SIDEBAR_WIDTH + MIN_CONTENT_WIDTH;
  return (
    <View
      testID="workspace-layout"
      style={[styles.root, { backgroundColor: colors.background }]}
      onLayout={(event) => setWidth(event.nativeEvent.layout.width)}
    >
      <View
        style={{ width: expanded ? SIDEBAR_WIDTH + insets.left : 0, overflow: "hidden" }}
        accessibilityElementsHidden={!expanded}
        importantForAccessibility={expanded ? "auto" : "no-hide-descendants"}
        pointerEvents={expanded ? "auto" : "none"}
      >
        {expanded ? <WorkspaceSidebar /> : null}
      </View>
      <ContentPane>
        {/* Insets relative to the pane, so a sidebar consumes the left safe
            area while the chat still respects the right edge and system bars. */}
        <SafeAreaProvider>{children}</SafeAreaProvider>
      </ContentPane>
    </View>
  );
}

const styles = StyleSheet.create({
  root: { flex: 1, flexDirection: "row" },
});

import { useState, type ReactNode } from "react";
import { StyleSheet, useWindowDimensions, View } from "react-native";
import Animated, { useAnimatedStyle } from "react-native-reanimated";

import { useScenePager } from "@/ui/ScenePager";
import { SegmentedSwitch } from "@/ui/SegmentedSwitch";

import { PROJECT_VIEWS, projectSegments, type ProjectView } from "./projectHeader";

/**
 * A project screen's pages, a new session then the project's sessions,
 * reached by the header switch or a swipe left. `key` is the project; another
 * starts back on the first page. Not `active` on a screen without them.
 */
export function useProjectPages(key: string, active = true) {
  const [viewFor, setViewFor] = useState<{ key: string; view: ProjectView } | null>(null);
  const view: ProjectView = viewFor?.key === key ? viewFor.view : "chat";
  const setView = (next: ProjectView) => setViewFor({ key, view: next });

  const { width } = useWindowDimensions();
  const progress = useScenePager(
    PROJECT_VIEWS.indexOf(view),
    (page) => setView(PROJECT_VIEWS[page] ?? "chat"),
    PROJECT_VIEWS.length,
    active,
  );
  const slide = useAnimatedStyle(() => ({
    transform: [{ translateX: -progress.get() * width }],
  }));
  return { view, setView, progress, slide };
}

export type ProjectPagesState = ReturnType<typeof useProjectPages>;

/** The header's switch between a project screen's pages. */
export function ProjectSwitch({ pages }: { pages: ProjectPagesState }) {
  return (
    <SegmentedSwitch<ProjectView>
      value={pages.view}
      onChange={pages.setView}
      progress={pages.progress}
      segments={projectSegments()}
    />
  );
}

/** The pages themselves, side by side, sliding with the switch. */
export function ProjectPages({
  pages,
  chat,
  sessions,
}: {
  pages: ProjectPagesState;
  /** The first page. */
  chat: ReactNode;
  /** The second. */
  sessions: ReactNode;
}) {
  const { width } = useWindowDimensions();
  return (
    <Animated.View
      style={[styles.pages, { width: width * PROJECT_VIEWS.length }, pages.slide]}
    >
      {/* The sessions come first in the view order, but laid out on the
          right: iOS blurs under the native header the scroll view it finds
          down each view's first child, and the list is what scrolls there. */}
      <View style={{ width }}>{sessions}</View>
      <View style={{ width }}>{chat}</View>
    </Animated.View>
  );
}

const styles = StyleSheet.create({
  pages: {
    flex: 1,
    flexDirection: "row-reverse",
    alignSelf: "flex-start",
  },
});

import { useState, type ReactNode } from "react";
import { StyleSheet, useWindowDimensions, View } from "react-native";
import Animated, { useAnimatedStyle } from "react-native-reanimated";

import { useScenePager } from "@/ui/ScenePager";
import { SegmentedSwitch } from "@/ui/SegmentedSwitch";

import { useInboxBadge } from "@/features/inbox/InboxProvider";

import { projectSegments, projectViews, type ProjectView } from "./projectHeader";

/**
 * A project screen's pages, a new session then the project's sessions,
 * reached by the header switch or a swipe left. `key` is the project; another
 * starts back on the first page. Not `active` on a screen without them.
 */
export function useProjectPages(key: string, active = true, inbox = false) {
  const views = projectViews(inbox);
  const [viewFor, setViewFor] = useState<{ key: string; view: ProjectView } | null>(null);
  const picked = viewFor?.key === key ? viewFor.view : "chat";
  // The Inbox page goes away with the desktop's support for it.
  const view: ProjectView = views.includes(picked) ? picked : "chat";
  const setView = (next: ProjectView) => setViewFor({ key, view: next });

  const { width } = useWindowDimensions();
  const progress = useScenePager(
    views.indexOf(view),
    (page) => setView(views[page] ?? "chat"),
    views.length,
    active,
  );
  const slide = useAnimatedStyle(() => ({
    transform: [{ translateX: -progress.get() * width }],
  }));
  return { view, setView, progress, slide, views, inbox };
}

export type ProjectPagesState = ReturnType<typeof useProjectPages>;

/** The header's switch between a project screen's pages. */
export function ProjectSwitch({
  pages,
  projectId,
}: {
  pages: ProjectPagesState;
  projectId: string;
}) {
  const unread = useInboxBadge(pages.inbox ? projectId : undefined);
  return (
    <SegmentedSwitch<ProjectView>
      value={pages.view}
      onChange={pages.setView}
      progress={pages.progress}
      // Selected, the Inbox says what it holds itself.
      segments={projectSegments(
        pages.inbox ? { unread: pages.view === "inbox" ? 0 : unread } : undefined,
      )}
    />
  );
}

/** The pages themselves, side by side, sliding with the switch. */
export function ProjectPages({
  pages,
  chat,
  sessions,
  inbox,
}: {
  pages: ProjectPagesState;
  /** The first page. */
  chat: ReactNode;
  /** The second. */
  sessions: ReactNode;
  /** The third, where the desktop has an Inbox. */
  inbox?: ReactNode;
}) {
  const { width } = useWindowDimensions();
  return (
    <Animated.View
      style={[styles.pages, { width: width * pages.views.length }, pages.slide]}
    >
      {/* The sessions come first in the view order, but laid out on the
          right: iOS blurs under the native header the scroll view it finds
          down each view's first child, and the list is what scrolls there. */}
      <View style={{ width, marginRight: pages.inbox ? width : 0 }}>{sessions}</View>
      <View style={{ width }}>{chat}</View>
      {/* The Inbox is the far right page, placed there rather than first so
          the sessions stay the first child. */}
      {pages.inbox && inbox ? (
        <View style={[styles.inbox, { width }]}>{inbox}</View>
      ) : null}
    </Animated.View>
  );
}

const styles = StyleSheet.create({
  inbox: {
    position: "absolute",
    top: 0,
    bottom: 0,
    right: 0,
  },
  pages: {
    flex: 1,
    flexDirection: "row-reverse",
    alignSelf: "flex-start",
  },
});

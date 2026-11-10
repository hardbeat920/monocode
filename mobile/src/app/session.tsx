import { Redirect, useLocalSearchParams } from "expo-router";

import { SessionFrame } from "@/features/sessions/SessionScreen";
import { ProjectScreen } from "@/features/workspace/ProjectScreen";
import { projectForRoute } from "@/features/workspace/routing";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { useNativeHeaderHeight } from "@/ui/useNativeHeaderHeight";

// A session a Mono started, opened from its card in the Mono's chat: the
// session's own screen, presented over the chat as a native modal, as a file
// opens (see the root layout).
export default function SessionSheetRoute() {
  const { projectId, sessionId, title } = useLocalSearchParams<{
    projectId: string;
    sessionId: string;
    /** The card's title, for a session the phone's list leaves out. */
    title?: string;
  }>();
  // A Mono can start one in any of the desktop's projects, picked for the
  // phone or not.
  const project = projectForRoute(useWorkspace().allProjects, projectId);
  const headerHeight = useNativeHeaderHeight();
  if (!project || !sessionId) return <Redirect href="/" />;
  return (
    <SessionFrame nativeHeaderHeight={headerHeight}>
      <ProjectScreen project={project} sessionId={sessionId} sheet title={title} />
    </SessionFrame>
  );
}

import { Redirect, useLocalSearchParams } from "expo-router";

import { SessionFrame } from "@/features/sessions/SessionScreen";
import { ProjectScreen } from "@/features/workspace/ProjectScreen";
import { projectForRoute } from "@/features/workspace/routing";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { useNativeHeaderHeight } from "@/ui/useNativeHeaderHeight";

// One of a project's sessions, pushed over the project: the stack's native,
// translucent header over its chat, which runs under it, and its own composer,
// so the whole screen slides in and out with the push (see ProjectScreen).
export default function SessionRoute() {
  const { projectId, sessionId } = useLocalSearchParams<{
    projectId: string;
    sessionId: string;
  }>();
  const project = projectForRoute(useWorkspace().projects, projectId);
  const headerHeight = useNativeHeaderHeight();
  if (!project) return <Redirect href="/" />;
  return (
    <SessionFrame nativeHeaderHeight={headerHeight}>
      <ProjectScreen project={project} sessionId={sessionId} />
    </SessionFrame>
  );
}

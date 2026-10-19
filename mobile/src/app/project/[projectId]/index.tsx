import { Redirect, useLocalSearchParams, useNavigation } from "expo-router";
import { useEffect } from "react";

import { SessionFrame } from "@/features/sessions/SessionScreen";
import { onProjectPick } from "@/features/workspace/projectPick";
import { ProjectScreen } from "@/features/workspace/ProjectScreen";
import { projectForRoute } from "@/features/workspace/routing";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { useNativeHeaderHeight } from "@/ui/useNativeHeaderHeight";

// A project: a new session, and its sessions beside it, under the stack's
// native header (see ProjectScreen).
export default function ProjectRoute() {
  const { projectId } = useLocalSearchParams<{ projectId: string }>();
  const project = projectForRoute(useWorkspace().projects, projectId);
  const headerHeight = useNativeHeaderHeight();
  // The project picker's pick. Its own route, so the pick lands here while
  // the picker is still over it.
  const navigation = useNavigation<{
    setParams: (params: { projectId: string }) => void;
  }>();
  useEffect(
    () => onProjectPick((id) => navigation.setParams({ projectId: id })),
    [navigation],
  );
  if (!project) return <Redirect href="/" />;
  return (
    <SessionFrame nativeHeaderHeight={headerHeight}>
      <ProjectScreen project={project} sessionId={undefined} />
    </SessionFrame>
  );
}

import { useLocalSearchParams } from "expo-router";

import { FileScreen } from "@/features/files/FileScreen";
import { projectForRoute } from "@/features/workspace/routing";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";

// A project file, presented as a native modal (see the root layout).
export default function FileRoute() {
  const { projectId, path, worktreePath } = useLocalSearchParams<{
    projectId: string;
    path: string;
    worktreePath?: string;
  }>();
  // Router params arrive decoded, so the project's ID may come back as its
  // path; requests need the catalog's ID.
  const { projects } = useWorkspace();
  const project = projectForRoute(projects, projectId);
  return (
    <FileScreen
      projectId={project?.id ?? projectId}
      path={path}
      worktreePath={worktreePath || undefined}
    />
  );
}

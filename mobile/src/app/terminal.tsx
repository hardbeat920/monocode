import { Redirect, useLocalSearchParams } from "expo-router";

import { projectForRoute } from "@/features/workspace/routing";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { TerminalScreen } from "@/features/terminal/TerminalScreen";

// A shell on the desktop, in a native modal over the session, as Changes is
// (see the root layout). `run` is a saved action to type and run on opening.
export default function TerminalRoute() {
  const { projectId, worktreePath, run } = useLocalSearchParams<{
    projectId: string;
    worktreePath?: string;
    run?: string;
  }>();
  const project = projectForRoute(useWorkspace().projects, projectId);
  if (!project) return <Redirect href="/" />;
  return (
    <TerminalScreen
      project={project}
      worktreePath={worktreePath || undefined}
      run={run || undefined}
    />
  );
}

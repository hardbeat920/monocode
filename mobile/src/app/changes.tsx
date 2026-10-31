import { Redirect, router, Stack, useLocalSearchParams } from "expo-router";
import { useHeaderHeight } from "expo-router/react-navigation";
import { useState } from "react";
import { StyleSheet } from "react-native";
import { GestureHandlerRootView } from "react-native-gesture-handler";

import { ChangesView } from "@/features/changes/ChangesView";
import { useProjectChanges } from "@/features/connection/useProjectChanges";
import {
  CodeHeaderTitle,
  HeaderBlur,
  HeaderButton,
} from "@/features/files/codeScreen";
import { SheetLayout } from "@/features/sessions/SessionScreen";
import type { Project } from "@/features/workspace/types";
import { projectForRoute } from "@/features/workspace/routing";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { useColors } from "@/ui/theme";

// A session's working copy, staged and committed from a native modal over it,
// as the desktop's Changes panel (see the root layout).

/** Files read at a time; more are read as the list nears its end. */
const PAGE = 300;
export default function ChangesRoute() {
  const { projectId, worktreePath, harness } = useLocalSearchParams<{
    projectId: string;
    worktreePath?: string;
    harness?: string;
  }>();
  // The route decodes the project's ID back to its path; the desktop wants the ID.
  const project = projectForRoute(useWorkspace().projects, projectId);
  if (!project) return <Redirect href="/" />;
  return (
    <ProjectChanges
      project={project}
      worktreePath={worktreePath || undefined}
      harness={harness || undefined}
    />
  );
}

function ProjectChanges({
  project,
  worktreePath,
  harness,
}: {
  project: Project;
  worktreePath?: string;
  harness?: string;
}) {
  const [limit, setLimit] = useState(PAGE);
  const changes = useProjectChanges(project.id, true, worktreePath, limit);
  const files = changes.result?.files ?? [];
  const headerHeight = useHeaderHeight();
  const colors = useColors();
  return (
    // A modal is its own native view hierarchy, so its gestures need their own root.
    <GestureHandlerRootView style={styles.root}>
      <Stack.Screen
        options={{
          headerTitle: () => (
            <CodeHeaderTitle name="Changes">
              {changes.result?.branch || project.branch || project.name}
            </CodeHeaderTitle>
          ),
          headerLeft: () => (
            <HeaderButton icon="close" label="Close" onPress={() => router.back()} />
          ),
        }}
      />
      <SheetLayout top={headerHeight}>
        <ChangesView
          projectId={project.id}
          worktreePath={worktreePath}
          active
          files={files}
          summary={changes.summary}
          hasMore={files.length < changes.summary.files}
          // Once the page asked for has come in, the next one.
          onEndReached={() => setLimit((current) => (files.length >= current ? current + PAGE : current))}
          loading={!changes.result && !changes.error}
          error={changes.error}
          onChanged={changes.reload}
          harness={harness}
        />
      </SheetLayout>
      <HeaderBlur color={colors.background} />
    </GestureHandlerRootView>
  );
}

const styles = StyleSheet.create({
  root: {
    flex: 1,
  },
});

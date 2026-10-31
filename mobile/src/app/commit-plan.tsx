import { Redirect, router, Stack, useLocalSearchParams } from "expo-router";
import { useHeaderHeight } from "expo-router/react-navigation";
import { ActivityIndicator, StyleSheet, View } from "react-native";
import { GestureHandlerRootView } from "react-native-gesture-handler";

import { CommitPlanView } from "@/features/changes/CommitPlanView";
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

// A session's uncommitted work split into commits, then pushed and opened as a
// pull request, from a native modal over the session (see the root layout).
export default function CommitPlanRoute() {
  const { projectId, worktreePath, harness } = useLocalSearchParams<{
    projectId: string;
    worktreePath?: string;
    harness?: string;
  }>();
  const project = projectForRoute(useWorkspace().projects, projectId);
  if (!project) return <Redirect href="/" />;
  return (
    <ProjectCommitPlan
      project={project}
      worktreePath={worktreePath || undefined}
      harness={harness || undefined}
    />
  );
}

function ProjectCommitPlan({
  project,
  worktreePath,
  harness,
}: {
  project: Project;
  worktreePath?: string;
  harness?: string;
}) {
  // Only counted: the plan reads the files on the desktop.
  const changes = useProjectChanges(project.id, true, worktreePath, 0);
  const headerHeight = useHeaderHeight();
  const colors = useColors();
  return (
    // A modal is its own native view hierarchy, so its gestures need their own root.
    <GestureHandlerRootView style={styles.root}>
      <Stack.Screen
        options={{
          headerTitle: () => (
            <CodeHeaderTitle name="Commit plan">
              {changes.result?.branch || project.branch || project.name}
            </CodeHeaderTitle>
          ),
          headerLeft: () => (
            <HeaderButton icon="close" label="Close" onPress={() => router.back()} />
          ),
        }}
      />
      <SheetLayout top={headerHeight}>
        {changes.result ? (
          <CommitPlanView
            projectId={project.id}
            worktreePath={worktreePath}
            harness={harness}
            files={changes.summary.files}
            onChanged={changes.reload}
          />
        ) : (
          <View style={[styles.root, styles.loading]}>
            {changes.error ? null : <ActivityIndicator color={colors.muted} />}
          </View>
        )}
      </SheetLayout>
      <HeaderBlur color={colors.background} />
    </GestureHandlerRootView>
  );
}

const styles = StyleSheet.create({
  root: {
    flex: 1,
  },
  loading: {
    alignItems: "center",
    justifyContent: "center",
  },
});

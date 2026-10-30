import { router, useGlobalSearchParams } from "expo-router";
import { useState, type ReactNode } from "react";
import {
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
  useWindowDimensions,
} from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { ProjectMascot } from "@/features/mascots/ProjectMascot";
import { SessionsPage } from "@/features/sessions/SessionsPage";
import { SheetLayout } from "@/features/sessions/SessionScreen";
import { Icon } from "@/ui/Icon";
import { PaneWidth } from "@/ui/PaneWidth";
import { themedStyles } from "@/ui/theme";
import { projectForRoute } from "./routing";
import { workspaceLayout } from "./workspaceSizing";
import { useWorkspace } from "./WorkspaceProvider";

import {
  WorkspaceSidebarContext,
  useWorkspaceSidebar,
} from "./WorkspaceSidebarContext";

/** The navigator stays mounted when the window narrows or the sidebar closes,
 * preserving the active conversation, composer draft and navigation history. */
export function WorkspaceLayout({ children }: { children: ReactNode }) {
  const { width } = useWindowDimensions();
  const [collapsed, setCollapsed] = useState(false);
  const { wide, sidebarWidth, paneWidth } = workspaceLayout(width, collapsed);
  const styles = useStyles();
  return (
    <WorkspaceSidebarContext
      value={{
        wide,
        open: sidebarWidth > 0,
        toggle: () => setCollapsed((value) => !value),
      }}
    >
      <View style={styles.root}>
        {sidebarWidth > 0 ? <WorkspaceSidebar /> : null}
        <PaneWidth value={paneWidth}>
          <View style={styles.main}>{children}</View>
        </PaneWidth>
      </View>
    </WorkspaceSidebarContext>
  );
}

function WorkspaceSidebar() {
  const styles = useStyles();
  const insets = useSafeAreaInsets();
  const { projects } = useWorkspace();
  const { projectId, sessionId } = useGlobalSearchParams<{
    projectId?: string;
    sessionId?: string;
  }>();
  const project = projectForRoute(projects, projectId);
  const sidebar = useWorkspaceSidebar();
  return (
    <View
      accessibilityLabel="Workspace sidebar"
      style={[
        styles.sidebar,
        { paddingTop: insets.top + 8, paddingLeft: insets.left },
      ]}
    >
      <View style={styles.heading}>
        <Text accessibilityRole="header" style={styles.headingText}>
          Projects
        </Text>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Hide sidebar"
          onPress={sidebar.toggle}
          style={styles.button}
        >
          <Icon name="panel" size={22} />
        </Pressable>
      </View>
      <ScrollView
        style={styles.projects}
        keyboardShouldPersistTaps="handled"
        contentContainerStyle={styles.projectList}
      >
        {projects.map((item) => (
          <Pressable
            key={item.id}
            accessibilityRole="button"
            accessibilityLabel={item.name}
            accessibilityState={{ selected: item.id === project?.id }}
            onPress={() =>
              router.navigate({
                pathname: "/project/[projectId]",
                params: { projectId: item.id },
              })
            }
            style={({ pressed }) => [
              styles.project,
              item.id === project?.id && styles.selected,
              pressed && styles.pressed,
            ]}
          >
            <ProjectMascot
              project={item.path}
              name={item.mascot}
              color={item.color}
              size={22}
            />
            <Text style={styles.projectName} numberOfLines={1}>
              {item.name}
            </Text>
          </Pressable>
        ))}
      </ScrollView>
      {project ? (
        <>
          <View style={styles.heading}>
            <Text accessibilityRole="header" style={styles.headingText}>
              Sessions
            </Text>
            <Pressable
              accessibilityRole="button"
              accessibilityLabel="New session"
              style={styles.button}
              onPress={() =>
                router.navigate({
                  pathname: "/project/[projectId]",
                  params: { projectId: project.id },
                })
              }
            >
              <Icon name="add" size={22} />
            </Pressable>
          </View>
          <SheetLayout top={0}>
            <SessionsPage
              key={project.id}
              projectId={project.id}
              active
              selectedId={sessionId}
              compact
              onOpen={(session) => {
                if (session.id === sessionId) return;
                router.navigate({
                  pathname: "/project/[projectId]/session/[sessionId]",
                  params: { projectId: project.id, sessionId: session.id },
                });
              }}
            />
          </SheetLayout>
        </>
      ) : (
        <Text style={styles.hint}>Choose a project to see its sessions.</Text>
      )}
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  root: { flex: 1, flexDirection: "row", backgroundColor: colors.background },
  main: { flex: 1, minWidth: 0 },
  sidebar: {
    width: 320,
    borderRightWidth: StyleSheet.hairlineWidth,
    borderRightColor: colors.stroke,
  },
  heading: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    paddingLeft: 20,
    paddingRight: 8,
  },
  headingText: { fontSize: 15, fontWeight: "600", color: colors.muted },
  button: {
    width: 44,
    height: 44,
    alignItems: "center",
    justifyContent: "center",
  },
  projects: { maxHeight: 220, flexGrow: 0 },
  projectList: { paddingHorizontal: 10, paddingBottom: 12 },
  project: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
    padding: 12,
    borderRadius: 12,
  },
  projectName: { flex: 1, color: colors.content, fontSize: 16 },
  selected: { backgroundColor: colors.stroke },
  pressed: { opacity: 0.6 },
  hint: { color: colors.muted, padding: 20 },
}));

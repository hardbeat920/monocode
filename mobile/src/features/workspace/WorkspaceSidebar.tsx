import { router, useGlobalSearchParams } from "expo-router";
import { Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { ProjectMascot } from "@/features/mascots/ProjectMascot";
import { ProviderLogo } from "@/features/sessions/ProviderLogo";
import { Icon } from "@/ui/Icon";
import { themedStyles, useColors } from "@/ui/theme";

import { relativeTime } from "./relativeTime";
import { projectForRoute } from "./routing";
import { useWorkspace } from "./WorkspaceProvider";

const RECENT_LIMIT = 20;

/** Reuses the workspace catalog; opening the sidebar never starts a second watch. */
export function WorkspaceSidebar() {
  const styles = useStyles();
  const colors = useColors();
  const insets = useSafeAreaInsets();
  const { projects, sessions, monos } = useWorkspace();
  const params = useGlobalSearchParams<{ projectId?: string; sessionId?: string; monoId?: string }>();
  const selected = projectForRoute(projects, params.projectId);
  const recent = sessions
    .filter((session) => projects.some((project) => project.id === session.projectId))
    .sort((a, b) => b.updatedAt - a.updatedAt)
    .slice(0, RECENT_LIMIT);

  return (
    <View
      testID="workspace-sidebar"
      style={[styles.sidebar, { paddingTop: insets.top, paddingLeft: insets.left }]}
    >
      <View style={styles.header}>
        <Text accessibilityRole="header" style={styles.heading}>Projects</Text>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Settings"
          onPress={() => router.push("/settings")}
          style={styles.button}
        >
          <Icon name="settings" size={20} color={colors.muted} />
        </Pressable>
      </View>
      <ScrollView
        contentContainerStyle={[styles.list, { paddingBottom: insets.bottom + 16 }]}
        keyboardShouldPersistTaps="handled"
        showsVerticalScrollIndicator={false}
      >
        {projects.map((project) => (
          <Pressable
            key={project.id}
            accessibilityRole="button"
            accessibilityLabel={`Open project ${project.name}`}
            accessibilityState={{ selected: selected?.id === project.id }}
            onPress={() => router.navigate({
              pathname: "/project/[projectId]",
              params: { projectId: project.id },
            })}
            style={({ pressed }) => [styles.row, selected?.id === project.id && styles.selected, pressed && styles.pressed]}
          >
            <ProjectMascot project={project.path} name={project.mascot} color={project.color} size={22} />
            <Text numberOfLines={1} style={styles.title}>{project.name}</Text>
            <Icon name="compose" size={16} color={colors.muted} />
          </Pressable>
        ))}
        {!projects.length ? <Text style={styles.empty}>No projects selected.</Text> : null}

        {monos.length ? <Text accessibilityRole="header" style={styles.section}>Monos</Text> : null}
        {monos.map((mono) => (
          <Pressable
            key={mono.id}
            accessibilityRole="button"
            accessibilityLabel={`Open mono ${mono.name}`}
            accessibilityState={{ selected: params.monoId === mono.id }}
            onPress={() => router.navigate({ pathname: "/mono/[monoId]", params: { monoId: mono.id } })}
            style={({ pressed }) => [styles.row, params.monoId === mono.id && styles.selected, pressed && styles.pressed]}
          >
            <Icon name="bot" size={22} color={colors.muted} />
            <Text numberOfLines={1} style={styles.title}>{mono.name}</Text>
          </Pressable>
        ))}

        <Text accessibilityRole="header" style={styles.section}>Recent sessions</Text>
        {recent.map((session) => (
          <Pressable
            key={session.id}
            accessibilityRole="button"
            accessibilityLabel={`Open session ${session.title || "Untitled session"}`}
            accessibilityState={{ selected: params.sessionId === session.id }}
            onPress={() => {
              if (params.sessionId === session.id) return;
              router.navigate({
                pathname: "/project/[projectId]/session/[sessionId]",
                params: { projectId: session.projectId, sessionId: session.id },
              });
            }}
            style={({ pressed }) => [styles.row, params.sessionId === session.id && styles.selected, pressed && styles.pressed]}
          >
            <ProviderLogo harness={session.harness} size={18} color={colors.muted} />
            <View style={styles.session}>
              <Text numberOfLines={2} style={styles.sessionTitle}>{session.title || "Untitled session"}</Text>
              <Text numberOfLines={1} style={styles.meta}>
                {projects.find((project) => project.id === session.projectId)?.name}
                {" · "}{session.waiting ? "Needs attention" : session.busy ? "Working" : relativeTime(session.updatedAt)}
              </Text>
            </View>
            {session.busy || session.waiting ? (
              <View style={[styles.dot, { backgroundColor: session.waiting ? colors.caution : colors.accent }]} />
            ) : null}
          </Pressable>
        ))}
        {!recent.length ? <Text style={styles.empty}>No recent sessions.</Text> : null}
      </ScrollView>
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  sidebar: { flex: 1, backgroundColor: colors.drawer, borderRightWidth: StyleSheet.hairlineWidth, borderRightColor: colors.stroke },
  header: { flexDirection: "row", alignItems: "center", paddingLeft: 20, paddingRight: 8, paddingTop: 20, minHeight: 64 },
  heading: { flex: 1, color: colors.muted, fontSize: 12, fontWeight: "600" },
  button: { width: 44, height: 44, alignItems: "center", justifyContent: "center" },
  list: { paddingHorizontal: 10 },
  section: { color: colors.muted, fontSize: 12, fontWeight: "600", marginTop: 20, marginBottom: 8, marginHorizontal: 10 },
  row: { minHeight: 48, padding: 10, flexDirection: "row", alignItems: "center", gap: 10, borderRadius: 10, marginBottom: 2 },
  selected: { backgroundColor: colors.selection },
  pressed: { backgroundColor: colors.fill },
  title: { flex: 1, color: colors.content, fontSize: 15, fontWeight: "500" },
  session: { flex: 1, gap: 4 },
  sessionTitle: { color: colors.content, fontSize: 14, lineHeight: 19 },
  meta: { color: colors.muted, fontSize: 11 },
  dot: { width: 6, height: 6, borderRadius: 3 },
  empty: { color: colors.muted, fontSize: 13, marginHorizontal: 10, marginVertical: 12 },
}));

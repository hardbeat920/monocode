import { router } from "expo-router";
import { useCallback } from "react";
import { Alert, Pressable, Text, View } from "react-native";

import type {
  LaunchedSession,
  LaunchedSessionState,
} from "@/features/workspace/types";
import { projectForRoute } from "@/features/workspace/routing";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { Icon, type IconName } from "@/ui/Icon";
import { Shimmer } from "@/ui/Shimmer";
import { themedStyles, useColors, withAlpha, type Palette } from "@/ui/theme";

import { ProviderLogo } from "./ProviderLogo";

/**
 * Opens a session a Mono started over its chat, as a sheet, as a file opens.
 * It may run in any of the desktop's projects, picked for the phone or not;
 * only one the desktop no longer has can't open.
 */
export function useOpenLaunchedSession() {
  const { allProjects: projects } = useWorkspace();
  return useCallback(
    (projectId: string, sessionId: string, title?: string) => {
      const project = projectForRoute(projects, projectId);
      if (!project) {
        Alert.alert(
          "Open it on your desktop",
          "This session runs in a project the desktop no longer lists.",
        );
        return;
      }
      router.push({
        pathname: "/session",
        params: { projectId: project.id, sessionId, ...(title ? { title } : {}) },
      });
    },
    [projects],
  );
}

const STATUS: Record<
  LaunchedSessionState,
  { label: string; icon: IconName; color: (colors: Palette) => string }
> = {
  working: { label: "Working", icon: "terminal", color: (c) => withAlpha(c.content, 0.55) },
  input: { label: "Needs input", icon: "chat", color: () => "#fbbf24" },
  draft: { label: "Draft", icon: "edit", color: (c) => withAlpha(c.content, 0.45) },
  failed: { label: "Failed", icon: "minus", color: (c) => c.deletions },
  stopped: { label: "Stopped", icon: "minus", color: (c) => withAlpha(c.content, 0.45) },
  done: { label: "Done", icon: "check", color: () => "rgba(52,211,153,0.9)" },
};

/** The sessions a Mono started, as the desktop's cards under its reply. */
export function LaunchedSessions({ sessions }: { sessions: LaunchedSession[] }) {
  const styles = useStyles();
  return (
    <View style={styles.list}>
      {sessions.map((session) => (
        <LaunchedSessionCard key={session.id} session={session} />
      ))}
    </View>
  );
}

/**
 * A session the Mono started: a mark for how it stands, its title, shimmering
 * while it works, and the project it runs in. A tap opens it.
 */
function LaunchedSessionCard({ session }: { session: LaunchedSession }) {
  const styles = useStyles();
  const colors = useColors();
  const open = useOpenLaunchedSession();
  const status = STATUS[session.state] ?? STATUS.done;
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={`Open session ${session.title}, ${status.label.toLowerCase()}`}
      onPress={() => open(session.projectId, session.id, session.title)}
      style={({ pressed }) => [styles.card, pressed && styles.pressed]}
    >
      <Icon name={status.icon} size={16} color={status.color(colors)} />
      {session.state === "working" ? (
        <Shimmer style={styles.title}>{session.title}</Shimmer>
      ) : (
        <Text style={styles.title} numberOfLines={1}>
          {session.title}
        </Text>
      )}
      {/* Squeezed, the title gives way first and the project after it. */}
      <View style={styles.project}>
        <ProviderLogo harness={session.harness} size={14} />
        <Text style={styles.projectName} numberOfLines={1}>
          {session.project}
        </Text>
      </View>
    </Pressable>
  );
}

const useStyles = themedStyles((colors) => ({
  list: {
    alignItems: "flex-start",
    gap: 8,
    paddingHorizontal: 20,
    paddingTop: 4,
    paddingBottom: 12,
  },
  card: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    maxWidth: "100%",
    minHeight: 40,
    borderRadius: 12,
    borderWidth: 1,
    borderColor: colors.stroke,
    backgroundColor: withAlpha(colors.content, 0.03),
    paddingLeft: 12,
    paddingRight: 6,
    paddingVertical: 6,
  },
  pressed: {
    backgroundColor: withAlpha(colors.content, 0.07),
  },
  title: {
    flexShrink: 1,
    color: colors.content,
    fontSize: 14,
    lineHeight: 20,
  },
  project: {
    flexDirection: "row",
    alignItems: "center",
    gap: 4,
    flexShrink: 0,
    maxWidth: "45%",
    borderRadius: 999,
    borderWidth: 1,
    borderColor: colors.stroke,
    backgroundColor: withAlpha(colors.content, 0.05),
    paddingHorizontal: 8,
    paddingVertical: 3,
  },
  projectName: {
    flexShrink: 1,
    color: withAlpha(colors.content, 0.65),
    fontSize: 12,
    lineHeight: 16,
  },
}));

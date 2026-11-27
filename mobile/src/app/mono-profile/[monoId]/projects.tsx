import * as Haptics from "expo-haptics";
import { useLocalSearchParams } from "expo-router";
import { Fragment } from "react";
import { Alert, StyleSheet, Text, View } from "react-native";

import { useOffline } from "@/features/connection/ConnectionNotice";
import { ProjectMascot } from "@/features/mascots/ProjectMascot";
import { SettingsPage, SettingsRow } from "@/features/settings/SettingsUI";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { Icon } from "@/ui/Icon";
import { radii, themedStyles, useColors } from "@/ui/theme";

// Which projects a Mono works in: every project on the desktop, checked where
// it's assigned. A tap assigns or unassigns it at once, as the desktop's
// settings do; without the desktop there it can't be changed.
export default function MonoProjectsScreen() {
  const { monoId } = useLocalSearchParams<{ monoId: string }>();
  const { monos, allProjects, editMono } = useWorkspace();
  const mono = monos.find((m) => m.id === monoId);
  const offline = useOffline();
  const styles = useStyles();
  const colors = useColors();
  if (!mono) return null;
  const toggle = (projectId: string) => {
    void Haptics.selectionAsync();
    const assigned = mono.projectIds.includes(projectId);
    editMono(mono, assigned ? { removeProject: projectId } : { addProject: projectId }).catch(
      (reason: unknown) =>
        Alert.alert("Couldn’t save", reason instanceof Error ? reason.message : String(reason)),
    );
  };
  return (
    <SettingsPage>
      <View style={[styles.card, offline && styles.off]} pointerEvents={offline ? "none" : "auto"}>
        {allProjects.map((project, index) => (
          <Fragment key={project.id}>
            {index > 0 ? <View style={styles.divider} /> : null}
            <SettingsRow
              icon={<ProjectMascot project={project.id} color={project.color} name={project.mascot} />}
              label={project.name}
              showChevron={false}
              accessory={
                mono.projectIds.includes(project.id) ? (
                  <Icon name="check" size={18} color={colors.accent} />
                ) : null
              }
              onPress={() => toggle(project.id)}
            />
          </Fragment>
        ))}
      </View>
      <Text style={styles.note}>
        {offline
          ? "Connect to your desktop to change this Mono's projects."
          : `${mono.name} can start sessions and make changes in the projects checked here.`}
      </Text>
    </SettingsPage>
  );
}

const useStyles = themedStyles((colors) => ({
  card: {
    backgroundColor: colors.surface,
    borderRadius: radii.card,
    borderCurve: "continuous",
    paddingHorizontal: 10,
    paddingVertical: 6,
  },
  divider: {
    height: StyleSheet.hairlineWidth,
    marginLeft: 40,
    backgroundColor: colors.stroke,
  },
  off: {
    opacity: 0.4,
  },
  note: {
    color: colors.muted,
    fontSize: 13,
    lineHeight: 18,
    marginTop: 8,
    marginHorizontal: 16,
  },
}));

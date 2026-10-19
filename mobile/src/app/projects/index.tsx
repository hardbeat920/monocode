import { router, Stack, useLocalSearchParams } from "expo-router";
import { Fragment, useState } from "react";
import { ActivityIndicator, StyleSheet, Text, View } from "react-native";

import { ProjectMascot } from "@/features/mascots/ProjectMascot";
import { SettingsPage, SettingsRow } from "@/features/settings/SettingsUI";
import { pickProject } from "@/features/workspace/projectPick";
import { projectForRoute } from "@/features/workspace/routing";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { Icon } from "@/ui/Icon";
import { themedStyles, useColors } from "@/ui/theme";

/** Frames the picked row's spinner is drawn for before the switch takes JS. */
const SPINNER_FRAMES = 2;

// The project switch, from the header's menu button: every project, the one
// open checked, narrowed by the search in the header. Picking spins on the
// row while the screen under it switches, then closes.
export default function ProjectsScreen() {
  const colors = useColors();
  const styles = useStyles();
  const { current } = useLocalSearchParams<{ current?: string }>();
  const { projects } = useWorkspace();
  const selected = projectForRoute(projects, current);
  const [switching, setSwitching] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const needle = query.trim().toLowerCase();
  const shown = needle
    ? projects.filter((project) =>
        project.name.toLowerCase().includes(needle),
      )
    : projects;
  const pick = (id: string) => {
    if (switching) return;
    if (id === selected?.id) return router.back();
    setSwitching(id);
    afterFrames(SPINNER_FRAMES, () => {
      // A project screen switches itself under the sheet; any other is
      // replaced by the project's once the sheet is gone.
      if (!pickProject(id)) {
        router.back();
        return router.replace({
          pathname: "/project/[projectId]",
          params: { projectId: id },
        });
      }
      // Closed once the switched screen has been drawn under the sheet.
      afterFrames(2, () => router.back());
    });
  };
  return (
    <>
      <Stack.Screen
        options={{
          headerSearchBarOptions: {
            placeholder: "Search projects",
            hideWhenScrolling: false,
            autoCapitalize: "none",
            onChangeText: (event) => setQuery(event.nativeEvent.text),
          },
        }}
      />
      <SettingsPage>
        {shown.map((project, index) => (
          <Fragment key={project.id}>
            {index > 0 ? <View style={styles.divider} /> : null}
            <SettingsRow
              icon={
                <ProjectMascot
                  project={project.path}
                  name={project.mascot}
                  color={project.color}
                  size={20}
                />
              }
              label={project.name}
              accessory={
                switching === project.id ? (
                  <ActivityIndicator size="small" color={colors.muted} />
                ) : project.id === (switching ? null : selected?.id) ? (
                  <Icon name="check" size={18} color={colors.accent} />
                ) : null
              }
              onPress={() => pick(project.id)}
            />
          </Fragment>
        ))}
        {needle && !shown.length ? (
          <Text style={styles.empty}>No projects match.</Text>
        ) : null}
      </SettingsPage>
    </>
  );
}

function afterFrames(frames: number, then: () => void) {
  requestAnimationFrame(() =>
    frames <= 1 ? then() : afterFrames(frames - 1, then),
  );
}

const useStyles = themedStyles((colors) => ({
  divider: {
    height: StyleSheet.hairlineWidth,
    marginLeft: 40,
    backgroundColor: colors.stroke,
  },
  empty: {
    color: colors.muted,
    fontSize: 15,
    textAlign: "center",
    marginTop: 24,
  },
}));

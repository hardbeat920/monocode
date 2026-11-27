import * as Haptics from "expo-haptics";
import { useLocalSearchParams } from "expo-router";
import { Fragment } from "react";
import { Pressable, StyleSheet, Text, View } from "react-native";

import { ACCESS_MODES, accessMode } from "@/features/models/access";
import type { RuntimeMode } from "@/features/models/types";
import { useDefaultModels } from "@/features/settings/defaultModel";
import { SettingsPage } from "@/features/settings/SettingsUI";
import { projectForRoute } from "@/features/workspace/routing";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { Icon } from "@/ui/Icon";
import { themedStyles, useColors } from "@/ui/theme";

// Permissions, after the desktop's access picker: the mode new sessions from
// this phone start with. Left to the desktop, they start with its own.
export default function PermissionsScreen() {
  const colors = useColors();
  const styles = useStyles();
  const { projectId } = useLocalSearchParams<{ projectId?: string }>();
  const { projects } = useWorkspace();
  const project = projectForRoute(projects, projectId) ?? projects[0];
  const { defaults, setRuntimeMode } = useDefaultModels();
  const chosen = defaults.runtimeMode;
  const desktopMode = project?.defaultRuntimeMode
    ? accessMode(project.defaultRuntimeMode).label
    : undefined;
  const pick = (mode: RuntimeMode | null) => {
    if (mode === chosen) return;
    void Haptics.selectionAsync();
    setRuntimeMode(mode);
  };

  return (
    <SettingsPage>
      <Text style={styles.heading}>New sessions start with</Text>
      <View style={styles.list}>
        {ACCESS_MODES.map((mode, index) => {
          const selected = mode.id === chosen;
          // Full access shows in amber, as on desktop.
          const tint =
            mode.id === "full-access" ? colors.caution : colors.content;
          return (
            <Fragment key={mode.id}>
              {index > 0 ? <View style={styles.divider} /> : null}
              <Pressable
                accessibilityRole="button"
                accessibilityState={{ selected }}
                accessibilityLabel={mode.label}
                accessibilityHint={mode.hint}
                onPress={() => pick(mode.id)}
                style={({ pressed }) => [styles.row, pressed && styles.pressed]}
              >
                <View style={styles.icon}>
                  <Icon name={mode.icon} size={18} color={tint} />
                </View>
                <View style={styles.text}>
                  <Text style={[styles.label, { color: tint }]}>
                    {mode.label}
                  </Text>
                  <Text style={styles.hint}>{mode.hint}</Text>
                </View>
                {selected ? (
                  <Icon name="check" size={18} color={colors.accent} />
                ) : null}
              </Pressable>
            </Fragment>
          );
        })}
      </View>

      <Text style={styles.note}>
        {chosen
          ? "Sessions you have already started keep their own permissions."
          : `New sessions use the permissions set on the desktop${desktopMode ? `, ${desktopMode}` : ""}.`}
      </Text>

      {/* Only once a mode is picked: until then the desktop's is in use. */}
      {chosen ? (
        <Pressable
          accessibilityRole="button"
          onPress={() => pick(null)}
          style={({ pressed }) => [styles.button, pressed && styles.pressed]}
        >
          <Icon name="computer" size={18} color={colors.content} />
          <Text style={styles.buttonLabel}>Use desktop default</Text>
        </Pressable>
      ) : null}
    </SettingsPage>
  );
}

const useStyles = themedStyles((colors) => ({
  heading: {
    color: colors.muted,
    fontSize: 15,
    marginTop: 20,
    marginBottom: 8,
    marginLeft: 4,
  },
  // Icons in line with the heading over them, as on Providers.
  list: {
    marginLeft: -4,
    paddingRight: 4,
  },
  divider: {
    height: StyleSheet.hairlineWidth,
    backgroundColor: colors.stroke,
  },
  row: {
    flexDirection: "row",
    alignItems: "center",
    minHeight: 52,
    paddingVertical: 10,
  },
  pressed: {
    opacity: 0.6,
  },
  icon: {
    width: 36,
    marginRight: 4,
    alignItems: "center",
  },
  text: {
    flex: 1,
    gap: 2,
    marginRight: 8,
  },
  label: {
    fontSize: 17,
  },
  hint: {
    color: colors.muted,
    fontSize: 13,
    lineHeight: 18,
  },
  note: {
    color: colors.muted,
    fontSize: 13,
    lineHeight: 18,
    marginTop: 8,
    marginHorizontal: 4,
  },
  button: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
    gap: 8,
    height: 50,
    marginTop: 24,
    borderRadius: 25,
    borderCurve: "continuous",
    backgroundColor: colors.surface,
  },
  buttonLabel: {
    color: colors.content,
    fontSize: 17,
    fontWeight: "500",
  },
}));

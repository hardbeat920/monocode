import * as Haptics from "expo-haptics";
import { router, useLocalSearchParams } from "expo-router";
import { Fragment } from "react";
import {
  ActivityIndicator,
  Pressable,
  StyleSheet,
  Text,
  View,
} from "react-native";

import { useModelCatalog } from "@/features/models/useModelCatalog";
import { ProviderLogo } from "@/features/sessions/ProviderLogo";
import { useDefaultModels } from "@/features/settings/defaultModel";
import { SettingsPage, SettingsRow } from "@/features/settings/SettingsUI";
import { projectForRoute } from "@/features/workspace/routing";
import type { Harness } from "@/features/workspace/types";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { Icon } from "@/ui/Icon";
import { themedStyles, useColors } from "@/ui/theme";

// Providers, after the desktop's: which provider new sessions from this phone
// start with, and its model, picked on the provider's own page. Left to the
// desktop, they start as a session started there would.
export default function ProvidersScreen() {
  const colors = useColors();
  const styles = useStyles();
  const { projectId } = useLocalSearchParams<{ projectId?: string }>();
  const { projects } = useWorkspace();
  // The desktop lists models for a project: the open one, or the first.
  const project = projectForRoute(projects, projectId) ?? projects[0];
  const { catalog, error } = useModelCatalog(project?.id);
  const { defaults, setProvider } = useDefaultModels();

  const countFor = (harness: Harness) =>
    catalog?.models.filter((model) => model.harness === harness).length ?? 0;
  const useDesktop = () => {
    if (!defaults.harness) return;
    void Haptics.selectionAsync();
    setProvider(null);
  };
  const desktopModel = project?.defaultModelName ?? project?.defaultModel;
  const openModels = (harness: Harness) =>
    router.push({
      pathname: "/settings/models",
      params: { harness, ...(project ? { projectId: project.id } : {}) },
    });

  return (
    <SettingsPage>
      <Text style={styles.heading}>New sessions start with</Text>
      <View style={styles.list}>
        {catalog?.harnesses.map((harness, index) => {
          const selected = harness.id === defaults.harness;
          const count = countFor(harness.id);
          return (
            <Fragment key={harness.id}>
              {index > 0 ? <View style={styles.divider} /> : null}
              <SettingsRow
                icon={<ProviderLogo harness={harness.id} size={20} />}
                label={harness.label}
                value={
                  selected
                    ? defaults.models[harness.id]?.name
                    : `${count} ${count === 1 ? "model" : "models"}`
                }
                accessory={
                  selected ? (
                    <View style={styles.selected}>
                      <Icon name="check" size={18} color={colors.accent} />
                      <Icon
                        name="chevronRight"
                        size={18}
                        color={colors.faint}
                      />
                    </View>
                  ) : undefined
                }
                onPress={() => openModels(harness.id)}
              />
            </Fragment>
          );
        })}
        {!catalog && !error ? (
          <ActivityIndicator style={styles.loading} color={colors.muted} />
        ) : null}
      </View>

      <Text style={styles.note}>
        {defaults.harness
          ? "A model keeps its effort and other settings. Sessions you have already started keep their own."
          : `New sessions use the provider and model set on the desktop${desktopModel ? `, ${desktopModel}` : ""}.`}
      </Text>

      {/* Only once a provider is picked: until then the desktop's is in use. */}
      {defaults.harness ? (
        <Pressable
          accessibilityRole="button"
          onPress={useDesktop}
          style={({ pressed }) => [styles.button, pressed && styles.pressed]}
        >
          <Icon name="computer" size={18} color={colors.content} />
          <Text style={styles.buttonLabel}>Use desktop default</Text>
        </Pressable>
      ) : null}

      {error ? <Text style={styles.error}>{error}</Text> : null}
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
  // Logos in line with the heading over them: their 36pt slot centres a
  // 20pt logo 8pt in, so the list starts 4pt left of the page.
  list: {
    marginLeft: -4,
    paddingRight: 4,
  },
  divider: {
    height: StyleSheet.hairlineWidth,
    backgroundColor: colors.stroke,
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
  pressed: {
    opacity: 0.6,
  },
  buttonLabel: {
    color: colors.content,
    fontSize: 17,
    fontWeight: "500",
  },
  selected: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  loading: {
    paddingVertical: 14,
  },
  note: {
    color: colors.muted,
    fontSize: 13,
    lineHeight: 18,
    marginTop: 8,
    marginHorizontal: 4,
  },
  error: {
    color: colors.deletions,
    fontSize: 14,
    lineHeight: 21,
    marginTop: 16,
  },
}));

import * as Haptics from "expo-haptics";
import { Stack, useLocalSearchParams } from "expo-router";
import { Fragment, useState } from "react";
import { ActivityIndicator, StyleSheet, Switch, Text, View } from "react-native";

import {
  effortSetting,
  fastSetting,
  otherSettings,
  settingValue,
  withModel,
} from "@/features/models/modelChoice";
import { EffortControl } from "@/features/models/ModelPicker";
import type { ModelPick } from "@/features/models/types";
import { useModelCatalog } from "@/features/models/useModelCatalog";
import { useDefaultModels } from "@/features/settings/defaultModel";
import { SettingsPage, SettingsRow } from "@/features/settings/SettingsUI";
import { projectForRoute } from "@/features/workspace/routing";
import type { Harness } from "@/features/workspace/types";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { Icon } from "@/ui/Icon";
import { radii, themedStyles, useColors } from "@/ui/theme";

// A provider's models, from Providers: picking one starts new sessions from
// this phone on it, with its effort and other settings above the list. The
// header's search narrows the list by name, id or who serves it.
export default function ModelsScreen() {
  const colors = useColors();
  const styles = useStyles();
  const { projectId, harness } = useLocalSearchParams<{ projectId?: string; harness: Harness }>();
  const { projects } = useWorkspace();
  const project = projectForRoute(projects, projectId) ?? projects[0];
  const { catalog, error } = useModelCatalog(project?.id);
  const { defaults, setModel } = useDefaultModels();
  const [query, setQuery] = useState("");

  const label = catalog?.harnesses.find((entry) => entry.id === harness)?.label;
  const models = catalog?.models.filter((model) => model.harness === harness) ?? [];
  const needle = query.trim().toLowerCase();
  const shown = needle
    ? models.filter((model) =>
        [model.name, model.id, model.provider ?? ""].some((field) =>
          field.toLowerCase().includes(needle),
        ),
      )
    : models;
  // Only this provider's pick shows here, chosen or not.
  const saved = defaults.models[harness];
  const chosen = defaults.harness === harness ? saved : undefined;
  // The settings shown are this provider's pick's, or its first model's until
  // one is picked; changing one picks it.
  const current: ModelPick | undefined =
    saved?.pick ??
    (models[0] ? withModel({ harness, model: models[0].id, modelSettings: {} }, models[0]) : undefined);
  const model = models.find((entry) => entry.id === current?.model);
  const effort = effortSetting(model);
  const fast = fastSetting(model);
  const others = otherSettings(model);

  const save = (pick: ModelPick) =>
    setModel({ pick, name: models.find((entry) => entry.id === pick.model)?.name ?? pick.model });
  const pickModel = (id: string) => {
    const entry = models.find((candidate) => candidate.id === id);
    if (!entry || id === chosen?.pick.model) return;
    void Haptics.selectionAsync();
    // Settings carry over from the last pick here, where they still apply.
    save(withModel(saved?.pick ?? { harness, model: id, modelSettings: {} }, entry));
  };
  const setSetting = (id: string, value: string) => {
    if (!current) return;
    save({ ...current, modelSettings: { ...current.modelSettings, [id]: value } });
  };
  const check = <Icon name="check" size={18} color={colors.accent} />;

  return (
    <>
      <Stack.Screen
        options={{
          title: label ?? "Models",
          headerSearchBarOptions: {
            placeholder: "Search models",
            hideWhenScrolling: false,
            autoCapitalize: "none",
            onChangeText: (event) => setQuery(event.nativeEvent.text),
          },
        }}
      />
      <SettingsPage>
        {current && (effort || fast || others.length) ? (
          <>
            <View style={[styles.card, styles.settings]}>
              {effort ? (
                <EffortControl
                  label={effort.label}
                  options={effort.options}
                  value={settingValue(current, effort)}
                  onChange={(next) => setSetting(effort.id, next)}
                />
              ) : null}
              {fast ? (
                <SettingsRow
                  icon="automation"
                  label="Fast mode"
                  accessory={
                    <Switch
                      value={settingValue(current, fast.setting) === fast.on}
                      onValueChange={(on) => setSetting(fast.setting.id, on ? fast.on : fast.off)}
                      trackColor={{ true: colors.accent }}
                    />
                  }
                />
              ) : null}
            </View>
            {others.map((setting) => (
              <View key={setting.id}>
                <Text style={styles.heading}>{setting.label}</Text>
                <View style={styles.card}>
                  {setting.options.map((option) => (
                    <SettingsRow
                      key={option.value}
                      label={option.label}
                      showChevron={false}
                      accessory={settingValue(current, setting) === option.value ? check : null}
                      onPress={() => setSetting(setting.id, option.value)}
                    />
                  ))}
                </View>
              </View>
            ))}
            <Text style={styles.heading}>Models</Text>
          </>
        ) : null}

        {shown.length ? (
          <View style={styles.list}>
            {shown.map((entry, index) => (
              <Fragment key={entry.id}>
                {index > 0 ? <View style={styles.divider} /> : null}
                <SettingsRow
                  label={entry.name}
                  value={entry.provider}
                  showChevron={false}
                  accessory={entry.id === chosen?.pick.model ? check : null}
                  onPress={() => pickModel(entry.id)}
                />
              </Fragment>
            ))}
          </View>
        ) : catalog ? (
          <Text style={styles.empty}>
            {needle ? "No models match." : "No models from this provider yet."}
          </Text>
        ) : error ? (
          <Text style={styles.error}>{error}</Text>
        ) : (
          <ActivityIndicator style={styles.loading} color={colors.muted} />
        )}
      </SettingsPage>
    </>
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
  card: {
    backgroundColor: colors.surface,
    borderRadius: radii.card,
    paddingHorizontal: 10,
    paddingVertical: 4,
  },
  settings: {
    paddingTop: 14,
    paddingHorizontal: 14,
    gap: 6,
  },
  // In line with the headings over it.
  list: {
    paddingHorizontal: 4,
  },
  divider: {
    height: StyleSheet.hairlineWidth,
    backgroundColor: colors.stroke,
  },
  loading: {
    paddingVertical: 24,
  },
  empty: {
    color: colors.muted,
    fontSize: 15,
    textAlign: "center",
    marginTop: 24,
  },
  error: {
    color: colors.deletions,
    fontSize: 14,
    lineHeight: 21,
    marginTop: 16,
  },
}));

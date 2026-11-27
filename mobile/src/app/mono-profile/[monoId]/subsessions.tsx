import * as Haptics from "expo-haptics";
import { Stack, useLocalSearchParams } from "expo-router";
import { Fragment, useState } from "react";
import { ActivityIndicator, Alert, StyleSheet, Text, View } from "react-native";

import { isModel, useMonoSubsessions } from "@/features/models/useMonoSubsessions";
import type { CatalogModel } from "@/features/models/types";
import { ProviderLogo } from "@/features/sessions/ProviderLogo";
import { SettingsPage, SettingsRow } from "@/features/settings/SettingsUI";
import type { MonoSubsessions } from "@/features/workspace/types";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { Icon } from "@/ui/Icon";
import { radii, themedStyles, useColors } from "@/ui/theme";

// The model and effort for sessions a Mono starts without naming one, kept on
// the desktop: its own, or any provider's model, searched from the header.
export default function MonoSubsessionsScreen() {
  const { monoId } = useLocalSearchParams<{ monoId: string }>();
  const { monos, editMono } = useWorkspace();
  const mono = monos.find((m) => m.id === monoId);
  const colors = useColors();
  const styles = useStyles();
  const [query, setQuery] = useState("");
  const { catalog, error, inherited, harness, model, effort, effortValue, unsetEffort } =
    useMonoSubsessions(mono);
  if (!mono) return null;

  const save = (next: MonoSubsessions | null) => {
    void Haptics.selectionAsync();
    editMono(mono, { subsessions: next }).catch((reason: unknown) =>
      Alert.alert("Couldn’t save", reason instanceof Error ? reason.message : String(reason)),
    );
  };
  const set = mono.subsessions ?? {};
  // Another model's effort may not fit, so a new model starts on its default.
  const pickModel = (entry: CatalogModel) => {
    if (!inherited && model && entry.id === model.id && entry.harness === harness) return;
    save({ harness: entry.harness, model: entry.id });
  };
  const pickEffort = (value: string | undefined) => {
    if (value === effortValue) return;
    const next = { ...set, effort: value };
    save(next.harness || next.effort ? next : null);
  };

  const needle = query.trim().toLowerCase();
  const sections = (catalog?.harnesses ?? [])
    .map((entry) => ({
      ...entry,
      models: (catalog?.models ?? []).filter(
        (candidate) =>
          candidate.harness === entry.id &&
          (!needle ||
            [candidate.name, candidate.id, candidate.provider ?? ""].some((field) =>
              field.toLowerCase().includes(needle),
            )),
      ),
    }))
    .filter((section) => section.models.length > 0);
  const check = <Icon name="check" size={18} color={colors.accent} />;

  return (
    <>
      <Stack.Screen
        options={{
          headerSearchBarOptions: {
            placeholder: "Find a model",
            hideWhenScrolling: false,
            autoCapitalize: "none",
            onChangeText: (event) => setQuery(event.nativeEvent.text),
          },
        }}
      />
      <SettingsPage>
        <Text style={styles.note}>
          For sessions {mono.name} starts without naming a model. Ones that name their own keep it.
        </Text>

        {effort && !needle ? (
          <>
            <Text style={styles.heading}>{effort.label}</Text>
            <View style={styles.card}>
              {[{ value: undefined, label: unsetEffort }, ...effort.options].map((option, index) => (
                <Fragment key={option.value ?? ""}>
                  {index > 0 ? <View style={styles.divider} /> : null}
                  <SettingsRow
                    label={option.label}
                    showChevron={false}
                    accessory={option.value === effortValue ? check : null}
                    onPress={() => pickEffort(option.value)}
                  />
                </Fragment>
              ))}
            </View>
          </>
        ) : null}

        {!needle ? (
          <View style={[styles.card, styles.first]}>
            <SettingsRow
              icon="sparkle"
              label={`Same as ${mono.name}`}
              showChevron={false}
              accessory={inherited ? check : null}
              onPress={() => {
                if (!inherited) save(set.effort ? { effort: set.effort } : null);
              }}
            />
          </View>
        ) : null}

        {sections.map((section) => (
          <View key={section.id}>
            <View style={styles.sectionHeading}>
              <ProviderLogo harness={section.id} size={16} />
              <Text style={styles.sectionLabel}>{section.label}</Text>
            </View>
            <View style={styles.card}>
              {section.models.map((entry, index) => (
                <Fragment key={entry.id}>
                  {index > 0 ? <View style={styles.divider} /> : null}
                  <SettingsRow
                    label={entry.name}
                    value={entry.provider}
                    showChevron={false}
                    accessory={
                      !inherited && isModel(entry, harness, model?.id) ? check : null
                    }
                    onPress={() => pickModel(entry)}
                  />
                </Fragment>
              ))}
            </View>
          </View>
        ))}

        {catalog && needle && !sections.length ? (
          <Text style={styles.empty}>No models match.</Text>
        ) : !catalog && error ? (
          <Text style={styles.error}>{error}</Text>
        ) : !catalog ? (
          <ActivityIndicator style={styles.loading} color={colors.muted} />
        ) : null}
      </SettingsPage>
    </>
  );
}

const useStyles = themedStyles((colors) => ({
  note: {
    color: colors.muted,
    fontSize: 13,
    lineHeight: 18,
    marginHorizontal: 4,
    marginTop: 4,
  },
  heading: {
    color: colors.muted,
    fontSize: 15,
    marginTop: 20,
    marginBottom: 8,
    marginLeft: 4,
  },
  sectionHeading: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
    marginTop: 20,
    marginBottom: 8,
    marginLeft: 4,
  },
  sectionLabel: {
    color: colors.muted,
    fontSize: 15,
  },
  first: {
    marginTop: 20,
  },
  card: {
    backgroundColor: colors.surface,
    borderRadius: radii.card,
    borderCurve: "continuous",
    paddingHorizontal: 10,
    paddingVertical: 4,
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

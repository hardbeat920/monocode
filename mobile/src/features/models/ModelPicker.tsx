import { Host, Slider } from "@expo/ui/swift-ui";
import { accessibilityLabel, tint } from "@expo/ui/swift-ui/modifiers";
import * as Haptics from "expo-haptics";
import { router } from "expo-router";
import { Fragment, useState } from "react";
import {
  ActivityIndicator,
  Pressable,
  StyleSheet,
  Switch,
  Text,
  View,
} from "react-native";

import { ProviderLogo } from "@/features/sessions/ProviderLogo";
import { SettingsPage } from "@/features/settings/SettingsUI";
import { useDefaultModel } from "@/features/settings/defaultModel";
import { projectForRoute } from "@/features/workspace/routing";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { Icon, type IconName } from "@/ui/Icon";
import { withAlpha, themedStyles, useColors } from "@/ui/theme";

import {
  effortSetting,
  fastSetting,
  otherSettings,
  settingValue,
  withModel,
} from "./modelChoice";
import { ACCESS_MODES } from "./access";
import { useSessionChoice } from "./sessionChoice";
import type { CatalogModel, ModelPick, RuntimeMode } from "./types";
import { useModelCatalog } from "./useModelCatalog";
import { AccountPicker } from "./AccountPicker";
import { useAccountUsage } from "./useAccountUsage";

/** Efforts past "high" in amber, as the desktop's effort tiles warm up. */
const HOT_EFFORT = /^(xhigh|max|ultra|ultracode|ultrathink)$/;

/** An option with its choices, each with an icon and a line under it if it has them. */
type Option = {
  id: string;
  label: string;
  value: string;
  description?: string;
  choices: {
    value: string;
    label: string;
    hint?: string;
    icon?: IconName;
    /** Shown in amber: efforts past high, full access. */
    warm?: boolean;
  }[];
  onPick: (value: string) => void;
  /** Drawn as the effort slider, or as a switch between its two choices, off then on. */
  control?: "slider" | "switch";
};

/**
 * What the picker's two screens share: the desktop's models, the session's
 * choice, and the chosen model's options and access. Each change goes
 * straight to the session, or to the new one about to start.
 */
function useModelPicker(
  projectId: string | undefined,
  sessionId: string | undefined,
) {
  const { projects } = useWorkspace();
  const project = projectForRoute(projects, projectId);
  const { catalog, error } = useModelCatalog(project?.id, sessionId);
  const { chosen, access, accounts, pickModel, pickAccess, pickAccount } =
    useSessionChoice(project, sessionId);

  const choice = chosen?.pick ?? catalog?.current ?? null;
  const model = catalog?.models.find((entry) => entry.id === choice?.model);
  const effort = effortSetting(model);
  const fast = fastSetting(model);
  const others = otherSettings(model);
  // fx has no permissions, so the desktop shows no picker for it.
  const showAccess = choice?.harness !== "fx";
  const providerAccounts =
    catalog?.accounts?.filter(
      (account) => account.provider === choice?.harness,
    ) ?? [];
  const accountId = choice
    ? (accounts[choice.harness] ??
      (sessionId && catalog?.current.harness === choice.harness
        ? catalog.current.providerAccountId
        : catalog?.selectedAccounts?.[choice.harness]) ??
      "default")
    : "default";
  const selectAccount = (id: string) => {
    if (!choice || !project || id === accountId) return;
    const name = model?.name ?? choice.model;
    pickAccount(choice, name, id);
    if (sessionId)
      router.dismissTo({
        pathname: "/project/[projectId]",
        params: { projectId: project.id },
      });
  };

  // Only the model's part: permissions are set apart.
  const update = ({ harness, model, modelSettings }: ModelPick) => {
    const name =
      catalog?.models.find((entry) => entry.id === model)?.name ?? model;
    pickModel({ harness, model, modelSettings }, name);
  };
  const setSetting = (id: string, settingValue: string) => {
    if (!choice) return;
    update({
      ...choice,
      modelSettings: { ...choice.modelSettings, [id]: settingValue },
    });
  };
  const pick = (entry: CatalogModel) => {
    if (!choice || entry.id === choice.model) return;
    void Haptics.selectionAsync();
    update(withModel(choice, entry));
  };

  const options: Option[] = !choice
    ? []
    : [
        ...(effort
          ? [
              {
                id: effort.id,
                label: effort.label,
                value: settingValue(choice, effort),
                description: effort.description,
                choices: effort.options.map((option) => ({
                  ...option,
                  warm: HOT_EFFORT.test(option.value),
                })),
                control: "slider" as const,
                onPick: (next: string) => setSetting(effort.id, next),
              },
            ]
          : []),
        ...(fast
          ? [
              {
                id: fast.setting.id,
                label: "Fast mode",
                value: settingValue(choice, fast.setting),
                choices: [
                  {
                    value: fast.off,
                    label: "Off",
                    hint: "Standard speed and cost.",
                  },
                  {
                    value: fast.on,
                    label: "On",
                    hint: "Quicker replies, at a higher cost.",
                  },
                ],
                onPick: (next: string) => setSetting(fast.setting.id, next),
                control: "switch" as const,
              },
            ]
          : []),
        ...others.map((setting) => ({
          id: setting.id,
          label: setting.label,
          value: settingValue(choice, setting),
          description: setting.description,
          choices: setting.options,
          onPick: (next: string) => setSetting(setting.id, next),
        })),
        ...(showAccess
          ? [
              {
                id: "access",
                label: "Access",
                value: access,
                choices: ACCESS_MODES.map((mode) => ({
                  value: mode.id,
                  label: mode.label,
                  hint: mode.hint,
                  icon: mode.icon,
                  warm: mode.id === "full-access",
                })),
                onPick: (next: string) => pickAccess(next as RuntimeMode),
              },
            ]
          : []),
      ];
  // What the options button sums them up as: each one's choice, fast mode
  // only when on.
  const summary = options
    .map((option) =>
      fast && option.id === fast.setting.id
        ? option.value === fast.on
          ? "Fast"
          : null
        : option.choices.find((entry) => entry.value === option.value)?.label,
    )
    .filter(Boolean)
    .join(" · ");

  return {
    catalog,
    error,
    choice,
    model,
    pick,
    options,
    summary,
    fullAccess: showAccess && access === "full-access",
    providerAccounts,
    accountId,
    selectAccount,
    resolvedProjectId: project?.id,
  };
}

/**
 * The picker's first screen: a button to the chosen model's options, then
 * every provider's models in sections that fold away, the chosen provider's
 * open. A search, from the header, looks across every provider.
 */
export function ModelList({
  projectId,
  sessionId,
  query,
  onOpenOptions,
}: {
  projectId: string | undefined;
  sessionId: string | undefined;
  query: string;
  onOpenOptions: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  const defaultModel = useDefaultModel();
  const {
    catalog,
    error,
    choice,
    model,
    pick,
    options,
    summary,
    fullAccess,
    providerAccounts,
    accountId,
    selectAccount,
    resolvedProjectId,
  } = useModelPicker(projectId, sessionId);
  const usage = useAccountUsage(
    providerAccounts.length > 0 ? resolvedProjectId : undefined,
    choice?.harness,
  );
  // Sections opened or folded by hand; the rest follow the chosen provider.
  const [folded, setFolded] = useState<Record<string, boolean>>({});

  const needle = query.trim().toLowerCase();
  const searching = needle.length > 0;
  // Each provider with its models, or with its matches when searching.
  const sections =
    catalog?.harnesses
      .map((harness) => ({
        ...harness,
        models: catalog.models.filter(
          (entry) =>
            entry.harness === harness.id &&
            (!searching ||
              [entry.name, entry.id, entry.provider ?? ""].some((field) =>
                field.toLowerCase().includes(needle),
              )),
        ),
      }))
      .filter((section) => !searching || section.models.length > 0) ?? [];
  const isOpen = (harness: string) =>
    searching || !(folded[harness] ?? harness !== choice?.harness);
  const toggle = (harness: string) => {
    void Haptics.selectionAsync();
    setFolded((current) => ({ ...current, [harness]: isOpen(harness) }));
  };

  // Still the page's scroll view while loading: the header's search bar
  // settles on the first one it finds, and drops down if it comes in later.
  if (!catalog || !choice)
    return (
      <SettingsPage>
        <View style={styles.loading}>
          {error ? (
            <Text style={styles.muted}>{error}</Text>
          ) : (
            <ActivityIndicator color={colors.muted} />
          )}
        </View>
      </SettingsPage>
    );

  return (
    <SettingsPage
      onRefresh={providerAccounts.length > 0 ? usage.refresh : undefined}
      refreshing={usage.refreshing}
    >
      <View style={styles.body}>
        {!searching && providerAccounts.length > 0 ? (
          <View style={styles.block}>
            <Text style={[styles.heading, styles.cardHeading]}>Account</Text>
            <AccountPicker
              key={choice.harness}
              accounts={providerAccounts}
              value={accountId}
              existingSession={!!sessionId}
              onPick={selectAccount}
              usage={usage}
            />
          </View>
        ) : null}
        {searching || options.length === 0 ? null : (
          <View style={styles.block}>
            <Text style={[styles.heading, styles.cardHeading]}>
              Selected provider
            </Text>
            <Pressable
              accessibilityRole="button"
              accessibilityLabel={`Options for ${model?.name ?? choice.model}: ${summary}`}
              onPress={() => {
                void Haptics.selectionAsync();
                onOpenOptions();
              }}
              style={({ pressed }) => [
                styles.optionsButton,
                pressed && styles.pressed,
              ]}
            >
              <ProviderLogo harness={choice.harness} size={20} />
              <View style={styles.rowText}>
                <Text style={styles.currentName} numberOfLines={1}>
                  {model?.name ?? choice.model}
                </Text>
                <Text
                  style={[
                    styles.rowHint,
                    fullAccess && { color: colors.caution },
                  ]}
                  numberOfLines={1}
                >
                  {summary || "Options"}
                </Text>
              </View>
              <Text style={styles.optionsLabel}>Options</Text>
              <Icon name="chevronRight" size={14} color={colors.muted} />
            </Pressable>
          </View>
        )}

        {sections.length === 0 ? (
          <Text style={[styles.muted, styles.empty]}>No matching models.</Text>
        ) : null}
        <View>
          {sections.map((section, sectionIndex) => {
            const open = isOpen(section.id);
            return (
              <Fragment key={section.id}>
                {/* Folded sections are split by a line; an open one is a
                    card, which stands apart by itself. */}
                {sectionIndex > 0 &&
                !open &&
                !isOpen(sections[sectionIndex - 1].id) ? (
                  <View style={styles.providerDivider} />
                ) : null}
                <View
                  style={[styles.providerBlock, open && styles.providerCard]}
                >
                  <Pressable
                    accessibilityRole="button"
                    accessibilityState={{ expanded: open }}
                    disabled={searching}
                    onPress={() => toggle(section.id)}
                    style={styles.providerRow}
                  >
                    <ProviderLogo harness={section.id} size={18} />
                    <Text style={styles.providerLabel}>{section.label}</Text>
                    {/* Folded, a section says how much it holds. */}
                    {open ? null : (
                      <Text style={styles.providerCount}>
                        {section.models.length}
                      </Text>
                    )}
                    <View style={styles.spacer} />
                    {searching ? null : (
                      <View style={open ? styles.chevronOpen : undefined}>
                        <Icon
                          name="chevronDown"
                          size={16}
                          color={colors.muted}
                        />
                      </View>
                    )}
                  </Pressable>
                  {open ? (
                    <View>
                      <View style={styles.divider} />
                      {section.models.length === 0 ? (
                        <Text style={[styles.muted, styles.groupEmpty]}>
                          No models from this provider yet.
                        </Text>
                      ) : (
                        section.models.map((entry, index) => {
                          const selected = entry.id === choice.model;
                          return (
                            <Fragment key={entry.id}>
                              {index > 0 ? (
                                <View style={styles.divider} />
                              ) : null}
                              <Pressable
                                accessibilityRole="button"
                                accessibilityState={{ selected }}
                                onPress={() => pick(entry)}
                                style={({ pressed }) => [
                                  styles.row,
                                  pressed && styles.pressed,
                                ]}
                              >
                                <View style={styles.rowText}>
                                  <View style={styles.rowTitle}>
                                    <Text
                                      style={[
                                        styles.rowLabel,
                                        selected && styles.rowLabelSelected,
                                      ]}
                                      numberOfLines={1}
                                    >
                                      {entry.name}
                                    </Text>
                                    {entry.id === defaultModel?.pick.model ? (
                                      <View style={styles.badge}>
                                        <Text style={styles.badgeLabel}>
                                          Default
                                        </Text>
                                      </View>
                                    ) : null}
                                  </View>
                                  {entry.provider ? (
                                    <Text
                                      style={styles.rowHint}
                                      numberOfLines={1}
                                    >
                                      {entry.provider}
                                    </Text>
                                  ) : null}
                                </View>
                                {selected ? (
                                  <Icon
                                    name="check"
                                    size={18}
                                    color={colors.accent}
                                  />
                                ) : null}
                              </Pressable>
                            </Fragment>
                          );
                        })
                      )}
                    </View>
                  ) : null}
                </View>
              </Fragment>
            );
          })}
        </View>
      </View>
    </SettingsPage>
  );
}

/** The picker's options screen: every option of the chosen model, and access, each with its choices. */
export function ModelOptions({
  projectId,
  sessionId,
}: {
  projectId: string | undefined;
  sessionId: string | undefined;
}) {
  const colors = useColors();
  const styles = useStyles();
  const { options } = useModelPicker(projectId, sessionId);
  return (
    <SettingsPage>
      <View style={styles.body}>
        {options.map((option) =>
          option.control === "slider" ? (
            <View key={option.id} style={[styles.card, styles.control]}>
              <EffortControl
                label={option.label}
                options={option.choices}
                value={option.value}
                onChange={option.onPick}
              />
              {option.description ? (
                <Text style={styles.rowHint}>{option.description}</Text>
              ) : null}
            </View>
          ) : option.control === "switch" ? (
            <View key={option.id} style={[styles.card, styles.control]}>
              <View style={styles.toggleRow}>
                <Icon name="automation" size={18} color={colors.content} />
                <View style={styles.rowText}>
                  <Text style={styles.rowLabel}>{option.label}</Text>
                  <Text style={styles.rowHint}>{option.choices[1]?.hint}</Text>
                </View>
                <Switch
                  value={option.value === option.choices[1]?.value}
                  onValueChange={(on) =>
                    option.onPick(option.choices[on ? 1 : 0].value)
                  }
                  trackColor={{ true: colors.accent }}
                />
              </View>
            </View>
          ) : (
            <View key={option.id} style={styles.block}>
              <Text style={styles.heading}>{option.label}</Text>
              {option.description ? (
                <Text style={styles.pageNote}>{option.description}</Text>
              ) : null}
              <View style={styles.card}>
                {option.choices.map((entry, index) => {
                  const selected = entry.value === option.value;
                  const tone = entry.warm ? colors.caution : colors.content;
                  return (
                    <Fragment key={entry.value}>
                      {index > 0 ? <View style={styles.divider} /> : null}
                      <Pressable
                        accessibilityRole="button"
                        accessibilityState={{ selected }}
                        onPress={() => {
                          if (selected) return;
                          void Haptics.selectionAsync();
                          option.onPick(entry.value);
                        }}
                        style={({ pressed }) => [
                          styles.row,
                          pressed && styles.pressed,
                        ]}
                      >
                        {entry.icon ? (
                          <Icon name={entry.icon} size={18} color={tone} />
                        ) : null}
                        <View style={styles.rowText}>
                          <Text
                            style={[
                              styles.rowLabel,
                              { color: tone },
                              selected && styles.rowLabelSelected,
                            ]}
                          >
                            {entry.label}
                          </Text>
                          {entry.hint ? (
                            <Text style={styles.rowHint}>{entry.hint}</Text>
                          ) : null}
                        </View>
                        {selected ? (
                          <Icon name="check" size={18} color={colors.accent} />
                        ) : null}
                      </Pressable>
                    </Fragment>
                  );
                })}
              </View>
            </View>
          ),
        )}
      </View>
    </SettingsPage>
  );
}

/** The effort slider, its level named on the right and warming past high. */
export function EffortControl({
  label,
  options,
  value,
  onChange,
}: {
  label: string;
  options: { value: string; label: string }[];
  value: string;
  onChange: (value: string) => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  const index = Math.max(
    0,
    options.findIndex((option) => option.value === value),
  );
  const hot = HOT_EFFORT.test(options[index]?.value ?? "");
  return (
    <View style={styles.effort}>
      <View style={styles.effortHeader}>
        <Text style={styles.rowLabel}>{label}</Text>
        <Text style={[styles.effortValue, hot && { color: colors.caution }]}>
          {options[index]?.label}
        </Text>
      </View>
      {/* The native slider, so a drag along it is not taken by the scroll. */}
      <Host style={styles.effortSlider}>
        <Slider
          value={index}
          min={0}
          max={options.length - 1}
          step={1}
          onValueChange={(next) => {
            const option = options[Math.round(next)];
            if (!option || option.value === value) return;
            void Haptics.selectionAsync();
            onChange(option.value);
          }}
          modifiers={[
            tint(hot ? colors.caution : colors.accent),
            accessibilityLabel(label),
          ]}
        />
      </Host>
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  loading: {
    paddingTop: 40,
    alignItems: "center",
  },
  body: {
    gap: 20,
  },
  block: {
    gap: 8,
  },
  heading: {
    color: colors.muted,
    fontSize: 13,
    fontWeight: "600",
    paddingHorizontal: 4,
  },
  cardHeading: {
    paddingHorizontal: 12,
  },
  optionsButton: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
    minHeight: 64,
    paddingHorizontal: 12,
    paddingVertical: 12,
    borderRadius: 18,
    borderCurve: "continuous",
    backgroundColor: withAlpha(colors.content, 0.07),
  },
  optionsLabel: {
    color: colors.muted,
    fontSize: 15,
  },
  pageNote: {
    color: colors.muted,
    fontSize: 14,
    paddingHorizontal: 4,
  },
  // A section is always laid out as a card, only drawn when open, so its
  // heading stays put as it opens and the models join it in one card.
  providerBlock: {
    marginVertical: 4,
    paddingHorizontal: 8,
    borderRadius: 18,
    borderCurve: "continuous",
  },
  providerCard: {
    backgroundColor: withAlpha(colors.content, 0.07),
  },
  providerDivider: {
    height: StyleSheet.hairlineWidth,
    marginHorizontal: 12,
    backgroundColor: colors.stroke,
  },
  currentName: {
    color: colors.content,
    fontSize: 17,
    fontWeight: "600",
  },
  providerRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
    minHeight: 52,
    paddingHorizontal: 4,
  },
  providerLabel: {
    color: colors.content,
    fontSize: 15,
    fontWeight: "600",
  },
  providerCount: {
    color: colors.muted,
    fontSize: 13,
    fontVariant: ["tabular-nums"],
  },
  spacer: {
    flex: 1,
  },
  chevronOpen: {
    transform: [{ rotate: "180deg" }],
  },
  empty: {
    textAlign: "center",
    paddingTop: 24,
  },
  // Lined up with the headings above, as the rows are.
  groupEmpty: {
    paddingHorizontal: 4,
    paddingVertical: 12,
  },
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
    minHeight: 52,
    paddingHorizontal: 4,
    paddingVertical: 10,
  },
  rowTitle: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  rowLabelSelected: {
    fontWeight: "600",
  },
  badge: {
    paddingHorizontal: 7,
    paddingVertical: 2,
    borderRadius: 6,
    backgroundColor: withAlpha(colors.accent, 0.16),
  },
  badgeLabel: {
    color: colors.accent,
    fontSize: 12,
    fontWeight: "600",
  },
  pressed: {
    opacity: 0.5,
  },
  rowText: {
    flex: 1,
    gap: 2,
  },
  rowLabel: {
    color: colors.content,
    fontSize: 16,
  },
  rowHint: {
    color: colors.muted,
    fontSize: 13,
  },
  divider: {
    height: StyleSheet.hairlineWidth,
    marginHorizontal: 4,
    backgroundColor: colors.stroke,
  },
  // The options screen's controls and choices, each in a card as the open
  // providers are, its rows inset by its padding and their own.
  card: {
    paddingHorizontal: 12,
    borderRadius: 18,
    borderCurve: "continuous",
    backgroundColor: withAlpha(colors.content, 0.07),
  },
  // A card holding a control rather than rows.
  control: {
    paddingVertical: 14,
    paddingHorizontal: 16,
    gap: 8,
  },
  toggleRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
  },
  effort: {
    gap: 8,
  },
  effortHeader: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
  },
  effortSlider: {
    height: 44,
  },
  effortValue: {
    color: colors.accent,
    fontSize: 15,
    fontWeight: "600",
  },
  muted: {
    color: colors.muted,
    fontSize: 15,
  },
}));

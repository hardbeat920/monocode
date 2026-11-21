import * as Haptics from "expo-haptics";
import { Fragment, useEffect, useMemo, useState, type ReactNode } from "react";
import {
  ActivityIndicator,
  Alert,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";

import { dictationLocale } from "@/features/sessions/dictation";
import { dictationEngineInfo, engineFor } from "@/features/sessions/dictationEngines";
import {
  languageName,
  matchLanguage,
  type DictationLanguage,
  type DictationPreference,
  type EngineId,
  type EngineInfo,
} from "@/features/sessions/dictationLanguages";
import { useDictationPreference } from "@/features/settings/dictation";
import { SettingsPage } from "@/features/settings/SettingsUI";
import { Icon, type IconName } from "@/ui/Icon";
import { radii, themedStyles, useColors } from "@/ui/theme";

/** Past this many languages the list gets a filter field. */
const FILTER_FROM = 12;

const ENGINE_ICONS: Record<EngineId, IconName> = {
  apple: "mic",
  parakeet: "aiChip",
};

// Counts language checks, across screens: a check finishing after a newer
// pick or an engine change no longer owns the selection, even once this
// screen has closed and opened again.
let latestCheck = 0;
const nextCheck = () => (latestCheck += 1);
const isLatest = (token: number) => token === latestCheck;

// Dictation: the engine that turns speech into words, and the language it
// listens for, apart from the phone's own language. A language is saved once
// the engine has taken it, fetching its speech assets if need be.
export default function DictationScreen() {
  const colors = useColors();
  const styles = useStyles();
  const [preference, setPreference] = useDictationPreference();
  const [engines, setEngines] = useState<EngineInfo[] | null>(null);
  // The language being checked with the engine, while it is ("automatic"
  // for Automatic), shown as picked until the engine answers.
  const [checking, setChecking] = useState<string | null>(null);
  const [filter, setFilter] = useState("");

  // An empty change writes the preference as it is, so Try Again saves the
  // pick already shown rather than waiting for a different one.
  const save = (next: Partial<DictationPreference>): void =>
    void setPreference(next).catch(() =>
      Alert.alert("Couldn't save", "This choice applies until Monocode closes.", [
        { text: "OK", style: "cancel" },
        { text: "Try Again", onPress: () => save({}) },
      ]),
    );

  useEffect(() => {
    let live = true;
    void dictationEngineInfo().then((info) => {
      if (live) setEngines(info);
    });
    return () => {
      live = false;
    };
  }, []);

  const engine =
    engines?.find((candidate) => candidate.id === preference.engine && candidate.available) ??
    engines?.find((candidate) => candidate.available);
  const languages = useMemo(
    () =>
      [...(engine?.languages ?? [])].sort((a, b) => a.name.localeCompare(b.name)),
    [engine],
  );
  const shown = useMemo(() => {
    const query = filter.trim().toLowerCase();
    if (!query) return languages;
    return languages.filter((language) =>
      [language.name, language.nativeName ?? "", language.id].some((text) =>
        text.toLowerCase().includes(query),
      ),
    );
  }, [languages, filter]);

  // Matched in the engine's own order, as planDictation matches for
  // recording; the name-sorted list above is only for display.
  const matchIds = (engine?.languages ?? []).map((language) => language.id);
  // The picked language as the engine knows it, so "ru" checks "ru-RU".
  const selected =
    preference.language === null
      ? null
      : (matchLanguage(preference.language, matchIds) ?? preference.language);
  const unsupported =
    !!engine &&
    preference.language !== null &&
    languages.length > 0 &&
    !languages.some((language) => language.id === selected);

  const pickEngine = (id: EngineId) => {
    if (id === preference.engine) return;
    void Haptics.selectionAsync();
    nextCheck();
    setChecking(null);
    // Another engine's languages: a filter for this one could hide them all.
    setFilter("");
    save({ engine: id });
  };

  const pickLanguage = async (id: string | null) => {
    if (checking || (id === selected && !unsupported)) return;
    void Haptics.selectionAsync();
    // Automatic is checked with the locale recording will use: the phone's
    // language matched to one the engine lists, as planDictation does.
    const checked =
      id ??
      (engine?.automatic
        ? null
        : (matchLanguage(dictationLocale(), matchIds) ?? dictationLocale()));
    // Nothing for the engine to fetch or refuse: saved as it is.
    if (!engine || checked === null) {
      nextCheck();
      save({ language: id });
      return;
    }
    const token = nextCheck();
    setChecking(id ?? "automatic");
    try {
      await engineFor(engine.id)?.prepare(checked);
      if (isLatest(token)) save({ language: id });
    } catch (reason) {
      if (isLatest(token)) {
        Alert.alert(
          "Language unavailable",
          reason instanceof Error ? reason.message : String(reason),
        );
      }
    } finally {
      if (isLatest(token)) setChecking(null);
    }
  };

  if (!engines) {
    return (
      <SettingsPage>
        <ActivityIndicator style={styles.loading} color={colors.muted} />
      </SettingsPage>
    );
  }

  const automaticHint = engine?.automatic
    ? "Recognizes the language you speak"
    : `Phone language, ${languageName(dictationLocale(), languages)}`;
  // While a language is checked it shows as picked, with a spinner.
  const isPicked = (id: string | null) =>
    checking !== null ? checking === (id ?? "automatic") : id === selected;

  return (
    <SettingsPage>
      <Text style={[styles.heading, styles.firstHeading]}>Engine</Text>
      <View style={styles.card}>
        {engines.map((info, index) => (
          <Fragment key={info.id}>
            {index > 0 ? <View style={styles.divider} /> : null}
            <ChoiceRow
              icon={ENGINE_ICONS[info.id]}
              label={info.label}
              hint={
                !info.available
                  ? info.unavailableReason
                  : !info.ready
                    ? info.notReadyReason
                    : engineHint(info.id)
              }
              selected={info.id === engine?.id}
              disabled={!info.available}
              onPress={() => pickEngine(info.id)}
            />
          </Fragment>
        ))}
      </View>
      {engine && engine.id !== preference.engine ? (
        <Text style={styles.note}>
          {`${engines.find((info) => info.id === preference.engine)?.label ?? "The engine you picked"} isn't available here, so ${engine.label} is used.`}
        </Text>
      ) : null}

      {engine ? (
        <>
          <Text style={styles.heading}>Language</Text>
          {languages.length > FILTER_FROM || filter ? (
            <TextInput
              accessibilityLabel="Find a language"
              placeholder="Find a language"
              placeholderTextColor={colors.muted}
              value={filter}
              onChangeText={setFilter}
              autoCorrect={false}
              autoCapitalize="none"
              clearButtonMode="while-editing"
              style={styles.filter}
            />
          ) : null}
          {filter.trim() ? null : (
            <View style={[styles.card, styles.automaticCard]}>
              <ChoiceRow
                icon="sparkle"
                label="Automatic"
                hint={automaticHint}
                selected={isPicked(null)}
                checking={checking === "automatic"}
                onPress={() => void pickLanguage(null)}
              />
            </View>
          )}
          {shown.length > 0 ? (
            <View style={styles.card}>
              {shown.map((language, index) => (
                <Fragment key={language.id}>
                  {index > 0 ? <View style={[styles.divider, styles.textDivider]} /> : null}
                  <ChoiceRow
                    label={language.name}
                    hint={nativeHint(language)}
                    selected={isPicked(language.id)}
                    checking={checking === language.id}
                    onPress={() => void pickLanguage(language.id)}
                  />
                </Fragment>
              ))}
            </View>
          ) : filter.trim() ? (
            <Text style={styles.note}>{`No language matches “${filter.trim()}”.`}</Text>
          ) : null}
          {unsupported ? (
            <Text style={[styles.note, { color: colors.deletions }]}>
              {`${languageName(preference.language!)} isn't available for ${engine.label}. Choose another language or engine.`}
            </Text>
          ) : null}
          {languages.length === 0 ? (
            <Text style={styles.note}>
              {`${engine.label} can't list its languages on this device, so Automatic uses the phone's language.`}
            </Text>
          ) : null}
        </>
      ) : null}

      <Text style={styles.note}>
        {"Recording and transcription happen on this phone. Only the words go to your desktop, so dictation works however it's connected, or while it's away."}
      </Text>
    </SettingsPage>
  );
}

function engineHint(id: EngineId) {
  switch (id) {
    case "apple":
      return "Built into iOS, on this phone";
    case "parakeet":
      return "Open model on this phone's Neural Engine, offline";
  }
}

function nativeHint(language: DictationLanguage) {
  return language.nativeName && language.nativeName !== language.name
    ? language.nativeName
    : undefined;
}

/** A row in a settings card: an optional icon, a label over a hint, and a
 *  check when picked (a spinner while being checked). */
function ChoiceRow({
  icon,
  label,
  hint,
  selected,
  checking = false,
  disabled = false,
  onPress,
}: {
  icon?: IconName;
  label: string;
  hint?: string;
  selected: boolean;
  checking?: boolean;
  disabled?: boolean;
  onPress: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  let trailing: ReactNode = null;
  if (checking) trailing = <ActivityIndicator size="small" color={colors.muted} />;
  else if (selected) trailing = <Icon name="check" size={18} color={colors.accent} />;
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityState={{ selected, disabled, busy: checking }}
      // The hint is in the label: it says which language Automatic uses.
      accessibilityLabel={hint ? `${label}, ${hint}` : label}
      disabled={disabled}
      onPress={onPress}
      style={({ pressed }) => [styles.row, pressed && styles.pressed, disabled && styles.disabled]}
    >
      {icon ? (
        <View style={styles.icon}>
          <Icon name={icon} size={18} color={colors.content} />
        </View>
      ) : null}
      <View style={styles.text}>
        <Text style={styles.label} numberOfLines={1}>
          {label}
        </Text>
        {hint ? (
          <Text style={styles.hint} numberOfLines={2}>
            {hint}
          </Text>
        ) : null}
      </View>
      {trailing}
    </Pressable>
  );
}

// Grouped as the rest of Settings: a muted heading over a filled, rounded
// card, rows split by hairlines inset past their icons.
const useStyles = themedStyles((colors) => ({
  loading: {
    marginTop: 40,
  },
  heading: {
    color: colors.muted,
    fontSize: 15,
    marginTop: 24,
    marginBottom: 8,
    marginLeft: 16,
  },
  firstHeading: {
    marginTop: 8,
  },
  card: {
    backgroundColor: colors.surface,
    borderRadius: radii.card,
    borderCurve: "continuous",
    paddingHorizontal: 10,
    paddingVertical: 6,
  },
  automaticCard: {
    marginBottom: 12,
  },
  divider: {
    height: StyleSheet.hairlineWidth,
    marginLeft: 40,
    backgroundColor: colors.stroke,
  },
  textDivider: {
    marginLeft: 6,
  },
  row: {
    flexDirection: "row",
    alignItems: "center",
    minHeight: 52,
    paddingVertical: 8,
    paddingHorizontal: 6,
    borderRadius: radii.row,
    gap: 8,
  },
  pressed: {
    opacity: 0.6,
  },
  disabled: {
    opacity: 0.45,
  },
  icon: {
    width: 26,
    alignItems: "center",
  },
  text: {
    flex: 1,
    gap: 2,
  },
  label: {
    color: colors.content,
    fontSize: 17,
  },
  hint: {
    color: colors.muted,
    fontSize: 13,
    lineHeight: 18,
  },
  filter: {
    height: 40,
    marginBottom: 12,
    paddingHorizontal: 14,
    borderRadius: radii.card,
    borderCurve: "continuous",
    backgroundColor: colors.surface,
    color: colors.content,
    fontSize: 16,
  },
  note: {
    color: colors.muted,
    fontSize: 13,
    lineHeight: 18,
    marginTop: 8,
    marginHorizontal: 16,
  },
}));

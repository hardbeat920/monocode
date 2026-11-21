import * as Haptics from "expo-haptics";
import { Fragment, useEffect, useMemo, useState } from "react";
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
  type EngineId,
  type EngineInfo,
} from "@/features/sessions/dictationLanguages";
import { useDictationPreference } from "@/features/settings/dictation";
import { SettingsPage } from "@/features/settings/SettingsUI";
import { Icon } from "@/ui/Icon";
import { themedStyles, useColors } from "@/ui/theme";

/** Past this many languages the list gets a filter field. */
const FILTER_FROM = 12;

// Dictation: the engine that turns speech into words, and the language it
// listens for, apart from the phone's own language. A language picked here is
// checked with the engine at once, fetching its speech assets if need be.
export default function DictationScreen() {
  const colors = useColors();
  const styles = useStyles();
  const [preference, setPreference] = useDictationPreference();
  const [engines, setEngines] = useState<EngineInfo[] | null>(null);
  // The language being checked with the engine, while it is.
  const [checking, setChecking] = useState<string | null>(null);
  const [filter, setFilter] = useState("");

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

  // The picked language as the engine knows it, so "ru" checks "ru-RU".
  const selected =
    preference.language === null
      ? null
      : (matchLanguage(
          preference.language,
          languages.map((language) => language.id),
        ) ?? preference.language);
  const unsupported =
    !!engine &&
    preference.language !== null &&
    languages.length > 0 &&
    !languages.some((language) => language.id === selected);

  const pickEngine = (id: EngineId) => {
    if (id === preference.engine) return;
    void Haptics.selectionAsync();
    setPreference({ engine: id });
  };

  const pickLanguage = async (id: string | null) => {
    if (checking || (id === selected && !unsupported)) return;
    void Haptics.selectionAsync();
    const previous = preference.language;
    setPreference({ language: id });
    if (!engine) return;
    const checked = id ?? (engine.automatic ? null : dictationLocale());
    if (checked === null) return;
    setChecking(id ?? "automatic");
    try {
      await engineFor(engine.id)?.prepare(checked);
    } catch (reason) {
      setPreference({ language: previous });
      Alert.alert(
        "Language unavailable",
        reason instanceof Error ? reason.message : String(reason),
      );
    } finally {
      setChecking(null);
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
    ? "Recognizes the language you speak."
    : `The phone's language, ${languageName(dictationLocale(), languages)}.`;

  return (
    <SettingsPage>
      <Text style={styles.heading}>Engine</Text>
      <View style={styles.list}>
        {engines.map((info, index) => {
          const chosen = info.id === engine?.id;
          return (
            <Fragment key={info.id}>
              {index > 0 ? <View style={styles.divider} /> : null}
              <Pressable
                accessibilityRole="button"
                accessibilityState={{ selected: chosen, disabled: !info.available }}
                accessibilityLabel={info.label}
                disabled={!info.available}
                onPress={() => pickEngine(info.id)}
                style={({ pressed }) => [
                  styles.row,
                  pressed && styles.pressed,
                  !info.available && styles.disabled,
                ]}
              >
                <View style={styles.icon}>
                  <Icon name="mic" size={18} color={colors.content} />
                </View>
                <View style={styles.text}>
                  <Text style={styles.label}>{info.label}</Text>
                  <Text style={styles.hint}>
                    {!info.available
                      ? info.unavailableReason
                      : !info.ready
                        ? info.notReadyReason
                        : "On this phone, offline once its speech assets are downloaded."}
                  </Text>
                </View>
                {chosen ? <Icon name="check" size={18} color={colors.accent} /> : null}
              </Pressable>
            </Fragment>
          );
        })}
      </View>
      {engine && engine.id !== preference.engine ? (
        <Text style={styles.note}>
          {`${engines.find((info) => info.id === preference.engine)?.label ?? "The engine you picked"} isn't available here, so ${engine.label} is used.`}
        </Text>
      ) : null}

      {engine ? (
        <>
          <Text style={styles.heading}>Language</Text>
          {languages.length > FILTER_FROM ? (
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
          <View style={styles.list}>
            {filter.trim() ? null : (
              <LanguageRow
                label="Automatic"
                hint={automaticHint}
                selected={preference.language === null}
                checking={checking === "automatic"}
                onPress={() => void pickLanguage(null)}
              />
            )}
            {shown.map((language, index) => (
              <Fragment key={language.id}>
                {index > 0 || !filter.trim() ? <View style={styles.divider} /> : null}
                <LanguageRow
                  label={language.name}
                  hint={nativeHint(language)}
                  selected={language.id === selected}
                  checking={checking === language.id}
                  onPress={() => void pickLanguage(language.id)}
                />
              </Fragment>
            ))}
          </View>
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

function nativeHint(language: DictationLanguage) {
  return language.nativeName && language.nativeName !== language.name
    ? language.nativeName
    : undefined;
}

function LanguageRow({
  label,
  hint,
  selected,
  checking,
  onPress,
}: {
  label: string;
  hint?: string;
  selected: boolean;
  checking: boolean;
  onPress: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityState={{ selected, busy: checking }}
      accessibilityLabel={label}
      onPress={onPress}
      style={({ pressed }) => [styles.row, pressed && styles.pressed]}
    >
      <View style={styles.text}>
        <Text style={styles.label}>{label}</Text>
        {hint ? <Text style={styles.hint}>{hint}</Text> : null}
      </View>
      {checking ? (
        <ActivityIndicator size="small" color={colors.muted} />
      ) : selected ? (
        <Icon name="check" size={18} color={colors.accent} />
      ) : null}
    </Pressable>
  );
}

const useStyles = themedStyles((colors) => ({
  loading: {
    marginTop: 40,
  },
  heading: {
    color: colors.muted,
    fontSize: 15,
    marginTop: 20,
    marginBottom: 8,
    marginLeft: 4,
  },
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
    paddingLeft: 4,
  },
  pressed: {
    opacity: 0.6,
  },
  disabled: {
    opacity: 0.45,
  },
  icon: {
    width: 32,
    marginRight: 4,
    alignItems: "center",
  },
  text: {
    flex: 1,
    gap: 2,
    marginRight: 8,
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
    marginBottom: 8,
    paddingHorizontal: 12,
    borderRadius: 10,
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
    marginHorizontal: 4,
  },
}));

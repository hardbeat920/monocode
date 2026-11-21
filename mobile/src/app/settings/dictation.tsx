import * as Haptics from "expo-haptics";
import { Fragment, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
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
import {
  dictationEngineInfo,
  engineFor,
  PARAKEET_MODELS,
  parakeetDetects,
  parakeetLanguages,
  parakeetModelInfo,
  parakeetModels,
  type ParakeetModelInfo,
  type ParakeetProgress,
  type ParakeetStatus,
} from "@/features/sessions/dictationEngines";
import {
  isCancelledDownload,
  languageName,
  matchLanguage,
  type DictationLanguage,
  type DictationPreference,
  type EngineId,
  type EngineInfo,
  type ParakeetModelId,
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

  // Read again once a model is picked, downloaded or deleted.
  const [generation, setGeneration] = useState(0);
  useEffect(() => {
    let live = true;
    void dictationEngineInfo().then((info) => {
      if (live) setEngines(info);
    });
    return () => {
      live = false;
    };
  }, [generation]);
  const refreshEngines = () => setGeneration((value) => value + 1);

  const engine =
    engines?.find((candidate) => candidate.id === preference.engine && candidate.available) ??
    engines?.find((candidate) => candidate.available);
  // Parakeet's languages and detection follow the model picked now, not the
  // engine info read before it was picked.
  const model = engine?.id === "parakeet" ? parakeetModelInfo(preference.parakeetModel) : null;
  const detects = model ? parakeetDetects(model) : !!engine?.automatic;
  const engineName = model?.name ?? engine?.label;
  const languages = useMemo(
    () =>
      [...(model ? parakeetLanguages(model) : (engine?.languages ?? []))].sort((a, b) =>
        a.name.localeCompare(b.name),
      ),
    [engine, model],
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
  const matchIds = model
    ? [...model.languages]
    : (engine?.languages ?? []).map((language) => language.id);
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
  // Automatic without detection records in the phone's language, which the
  // engine may not take (a French phone with the English-only model).
  const automaticUnsupported =
    !!engine &&
    preference.language === null &&
    !detects &&
    languages.length > 0 &&
    matchLanguage(dictationLocale(), matchIds) === null;

  // A language check under way belongs to the engine and model it began
  // with: picking another one drops it.
  const dropCheck = () => {
    nextCheck();
    setChecking(null);
  };

  const pickEngine = (id: EngineId) => {
    if (id === preference.engine) return;
    void Haptics.selectionAsync();
    dropCheck();
    // Another engine's languages: a filter for this one could hide them all.
    setFilter("");
    save({ engine: id });
  };

  // The language stays as picked: one the new model doesn't take is marked
  // below, and still there for the engine that does.
  const pickModel = (id: ParakeetModelId) => save({ parakeetModel: id });

  const pickLanguage = async (id: string | null) => {
    if (checking || (id === selected && !unsupported)) return;
    if (!engine) return;
    void Haptics.selectionAsync();
    const ids = matchIds;
    // Automatic is checked with the locale recording will use: the phone's
    // language matched to one the engine lists, as planDictation does.
    const checked =
      id ?? (detects ? null : (matchLanguage(dictationLocale(), ids) ?? dictationLocale()));
    if (checked === null) {
      dropCheck();
      save({ language: id });
      return;
    }
    if (model || !engine.ready) {
      dropCheck();
      if (ids.length > 0 && !matchLanguage(checked, ids)) {
        Alert.alert(
          "Language unavailable",
          `${languageName(checked)} isn't available for ${engineName}.`,
        );
        return;
      }
      save({ language: id });
      return;
    }
    // Apple fetches the language's speech assets: saved once it has them, if
    // nothing else was picked meanwhile.
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

  const automaticHint = detects
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
      {engine?.id === "parakeet" ? (
        <ParakeetModels
          picked={preference.parakeetModel}
          onPick={pickModel}
          onChange={refreshEngines}
        />
      ) : null}
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
              {`${languageName(preference.language!)} isn't available for ${engineName}. Choose another language${model ? ", model" : ""} or engine.`}
            </Text>
          ) : null}
          {automaticUnsupported ? (
            <Text style={[styles.note, { color: colors.deletions }]}>
              {`Automatic uses the phone's language, ${languageName(dictationLocale())}, which ${engineName} doesn't take. Choose a language${model ? ", model" : ""} or engine.`}
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

/** The Parakeet models: which one dictation uses, and each one's download. */
function ParakeetModels({
  picked,
  onPick,
  onChange,
}: {
  picked: ParakeetModelId;
  onPick: (id: ParakeetModelId) => void;
  onChange: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  const [statuses, setStatuses] = useState<Partial<Record<ParakeetModelId, ParakeetStatus>>>({});
  const [progress, setProgress] = useState<ParakeetProgress | null>(null);
  // A screen that has closed leaves its download to this one.
  const mounted = useRef(true);
  useEffect(
    () => () => {
      mounted.current = false;
    },
    [],
  );
  // A failed read is shown as such, with a retry, not as nothing downloaded.
  const [unreadable, setUnreadable] = useState(false);
  const refresh = () =>
    void parakeetModels.statuses().then(
      (next) => {
        if (!mounted.current) return;
        setStatuses(next);
        setUnreadable(false);
      },
      () => {
        if (!mounted.current) return;
        // Nothing stale under the error: what was read before may be wrong now.
        setStatuses({});
        setUnreadable(true);
      },
    );
  useEffect(refresh, []);

  const downloadingId = progress?.model ??
    PARAKEET_MODELS.find((model) => statuses[model.id]?.downloading)?.id;

  // Starts a download, or follows one already running: the native side joins
  // it, so a screen opened mid-download sees its progress and its end.
  const download = async (id: ParakeetModelId) => {
    setProgress({ model: id, fraction: 0, phase: "listing" });
    let failure: unknown = null;
    try {
      await parakeetModels.download(id, (next) => {
        if (mounted.current) setProgress(next);
      });
    } catch (reason) {
      failure = reason;
    }
    // The screen's engine list learns the model is ready even when this
    // section has closed (another engine picked) while it downloaded.
    onChange();
    if (!mounted.current) return;
    setProgress(null);
    refresh();
    if (isCancelledDownload(failure)) return;
    if (failure) {
      Alert.alert(
        "Download failed",
        failure instanceof Error ? failure.message : String(failure),
      );
    } else {
      void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
    }
  };
  const running = PARAKEET_MODELS.find((model) => statuses[model.id]?.downloading)?.id;
  useEffect(() => {
    if (!running || progress) return;
    const follow = setTimeout(() => void download(running), 0);
    return () => clearTimeout(follow);
    // Only when a status read finds a download this screen didn't start.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [running]);

  const pick = (id: ParakeetModelId) => {
    if (id === picked) return;
    void Haptics.selectionAsync();
    // The model loaded for the last dictation leaves memory before the next
    // dictation loads this one (native side).
    onPick(id);
    onChange();
  };
  const remove = (model: ParakeetModelInfo) =>
    Alert.alert(`Delete ${model.name}?`, "You can download it again at any time.", [
      { text: "Cancel", style: "cancel" },
      {
        text: "Delete",
        style: "destructive",
        onPress: () => {
          void parakeetModels
            .remove(model.id)
            .catch((reason) => Alert.alert("Could not delete", String(reason)))
            .finally(() => {
              refresh();
              onChange();
            });
        },
      },
    ]);

  return (
    <>
      <Text style={styles.heading}>Model</Text>
      {unreadable ? (
        <Text style={[styles.note, styles.unreadable]}>
          {"Couldn't read which models are on this phone. "}
          <Text
            accessibilityRole="button"
            style={{ color: colors.accent }}
            onPress={() => {
              refresh();
              onChange();
            }}
          >
            Try Again
          </Text>
        </Text>
      ) : null}
      <View style={styles.card}>
        {PARAKEET_MODELS.map((model, index) => (
          <Fragment key={model.id}>
            {index > 0 ? <View style={[styles.divider, styles.textDivider]} /> : null}
            <ModelRow
              model={model}
              status={statuses[model.id]}
              selected={model.id === picked}
              progress={downloadingId === model.id ? (progress?.fraction ?? 0) : null}
              compiling={downloadingId === model.id && progress?.phase === "compiling"}
              busy={!!downloadingId}
              onPick={() => pick(model.id)}
              onDownload={() => void download(model.id)}
              onCancel={() => void parakeetModels.cancel()}
              onDelete={() => remove(model)}
            />
          </Fragment>
        ))}
      </View>
      <Text style={styles.note}>
        Downloaded once from Hugging Face, then dictation works offline. Only the picked model is
        loaded, and only while you dictate.
      </Text>
    </>
  );
}

function ModelRow({
  model,
  status,
  selected,
  progress,
  compiling,
  busy,
  onPick,
  onDownload,
  onCancel,
  onDelete,
}: {
  model: ParakeetModelInfo;
  status: ParakeetStatus | undefined;
  selected: boolean;
  /** 0 to 1 while this model downloads. */
  progress: number | null;
  compiling: boolean;
  /** Some model is downloading: one at a time. */
  busy: boolean;
  onPick: () => void;
  onDownload: () => void;
  onCancel: () => void;
  onDelete: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  const unsupported = status ? !status.supported : false;
  const percent = Math.round((progress ?? 0) * 100);
  const state =
    progress !== null
      ? compiling
        ? "Preparing for this phone, once. Keep Monocode open…"
        : `Downloading… ${percent}%`
      : unsupported
        ? "Needs iOS 18 or later"
        : status?.installed
          ? `Downloaded, ${formatSize(status.sizeBytes)}`
          : status
            ? `Not downloaded, ${model.size}`
            : model.size;

  let action: ReactNode;
  if (progress !== null) {
    action = (
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={`Cancel downloading ${model.name}`}
        hitSlop={6}
        onPress={onCancel}
        style={({ pressed }) => [styles.action, pressed && styles.pressed]}
      >
        <Icon name="close" size={16} color={colors.muted} />
      </Pressable>
    );
  } else if (status?.installed) {
    action = (
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={`Delete ${model.name}`}
        hitSlop={6}
        onPress={onDelete}
        style={({ pressed }) => [styles.action, pressed && styles.pressed]}
      >
        <Icon name="trash" size={17} color={colors.muted} />
      </Pressable>
    );
  } else if (status && !unsupported) {
    action = (
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={`Download ${model.name}`}
        disabled={busy}
        onPress={onDownload}
        style={({ pressed }) => [
          styles.getButton,
          busy && styles.disabled,
          pressed && styles.pressed,
        ]}
      >
        <Text style={[styles.getLabel, { color: colors.accent }]}>Get</Text>
      </Pressable>
    );
  }

  return (
    <View style={styles.modelRow}>
      <Pressable
        accessibilityRole="button"
        accessibilityState={{ selected, disabled: unsupported }}
        accessibilityLabel={`${model.name}, ${model.detail}, ${state}`}
        disabled={unsupported}
        onPress={onPick}
        style={({ pressed }) => [
          styles.modelPick,
          pressed && styles.pressed,
          unsupported && styles.disabled,
        ]}
      >
        <View style={styles.text}>
          <Text style={styles.label} numberOfLines={1}>
            {model.name}
          </Text>
          <Text style={styles.hint} numberOfLines={2}>
            {model.detail}
          </Text>
          <Text style={styles.hint} numberOfLines={1}>
            {state}
          </Text>
          {progress !== null ? (
            <View style={styles.track}>
              <View style={[styles.fill, { width: `${percent}%` }]} />
            </View>
          ) : null}
        </View>
        {selected ? <Icon name="check" size={18} color={colors.accent} /> : null}
      </Pressable>
      {action}
    </View>
  );
}

function formatSize(bytes: number) {
  return bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.round(bytes / 1e6)} MB`;
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
  unreadable: {
    marginTop: 0,
    marginBottom: 8,
  },
  modelRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 4,
  },
  modelPick: {
    flex: 1,
    flexDirection: "row",
    alignItems: "center",
    minHeight: 60,
    paddingVertical: 10,
    paddingHorizontal: 6,
    borderRadius: radii.row,
    gap: 8,
  },
  action: {
    width: 36,
    height: 36,
    alignItems: "center",
    justifyContent: "center",
  },
  getButton: {
    minWidth: 56,
    height: 30,
    paddingHorizontal: 14,
    marginRight: 4,
    borderRadius: 15,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.fill,
  },
  getLabel: {
    fontSize: 15,
    fontWeight: "600",
  },
  track: {
    height: 3,
    marginTop: 6,
    borderRadius: 1.5,
    overflow: "hidden",
    backgroundColor: colors.fill,
  },
  fill: {
    height: 3,
    backgroundColor: colors.accent,
  },
  note: {
    color: colors.muted,
    fontSize: 13,
    lineHeight: 18,
    marginTop: 8,
    marginHorizontal: 16,
  },
}));

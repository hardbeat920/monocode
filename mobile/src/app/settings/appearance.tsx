import { Host, Picker, Slider, Text as NativeText } from "@expo/ui/swift-ui";
import {
  accessibilityLabel,
  pickerStyle,
  tag,
  tint,
} from "@expo/ui/swift-ui/modifiers";
import * as Haptics from "expo-haptics";
import { useLocalSearchParams } from "expo-router";
import { Fragment, useState } from "react";
import {
  ActivityIndicator,
  Image,
  Pressable,
  StyleSheet,
  Switch,
  Text,
  View,
} from "react-native";

import {
  OPACITY_RANGE,
  useEmptyBackground,
} from "@/features/background/useEmptyBackground";
import {
  useAppearancePreference,
  type AppearancePreference,
} from "@/features/settings/appearance";
import { SettingsPage, SettingsRow } from "@/features/settings/SettingsUI";
import { useCollapsedToolCalls } from "@/features/settings/toolCalls";
import { projectForRoute } from "@/features/workspace/routing";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { Icon } from "@/ui/Icon";
import {
  paletteFor,
  radii,
  setTheme,
  themedStyles,
  THEMES,
  useColors,
  useTheme,
  type ThemeName,
} from "@/ui/theme";

// Appearance: the open project's empty-session background first, shown as it
// will look, then light, dark or the phone's own as a native segmented
// control, the theme both are drawn in as a list, and how the transcript
// shows tool calls.
export default function AppearanceScreen() {
  const styles = useStyles();
  const { projectId } = useLocalSearchParams<{ projectId?: string }>();
  const project = projectForRoute(useWorkspace().projects, projectId);
  return (
    <SettingsPage>
      {project ? (
        <>
          <Text style={[styles.heading, styles.firstHeading]}>
            Background image
          </Text>
          <BackgroundSection projectId={project.id} />
        </>
      ) : null}
      <Text style={[styles.heading, !project && styles.firstHeading]}>
        Color scheme
      </Text>
      <SchemeControl />
      <ThemeList />
      <TranscriptSection />
    </SettingsPage>
  );
}

const SCHEMES = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
] as const satisfies { value: AppearancePreference; label: string }[];

/** Light, dark or the phone's own, as iOS's own segmented control. */
function SchemeControl() {
  const [preference, setPreference] = useAppearancePreference();
  return (
    <Host matchContents={{ vertical: true }}>
      <Picker
        selection={preference}
        onSelectionChange={(value) => {
          void Haptics.selectionAsync();
          setPreference(value as AppearancePreference);
        }}
        modifiers={[pickerStyle("segmented")]}
      >
        {SCHEMES.map((option) => (
          <NativeText key={option.value} modifiers={[tag(option.value)]}>
            {option.label}
          </NativeText>
        ))}
      </Picker>
    </Host>
  );
}

const THEME_NAMES = Object.keys(THEMES) as ThemeName[];

function ThemeList() {
  const colors = useColors();
  const styles = useStyles();
  const theme = useTheme();
  // The tapped row checks at once; the app redraws in its theme a frame
  // later, so the tap answers before that work starts.
  const [tapped, setTapped] = useState<ThemeName | null>(null);
  const shown = tapped ?? theme;
  const pick = (name: ThemeName) => {
    if (name === shown) return;
    void Haptics.selectionAsync();
    setTapped(name);
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        setTheme(name);
        setTapped(null);
      }),
    );
  };
  return (
    <>
      <Text style={styles.heading}>Theme</Text>
      <View style={styles.card}>
        {THEME_NAMES.map((name, index) => (
          <Fragment key={name}>
            {index > 0 ? <View style={styles.divider} /> : null}
            <SettingsRow
              icon={<Swatch name={name} />}
              label={THEMES[name].label}
              showChevron={false}
              accessory={
                name === shown ? (
                  <Icon name="check" size={18} color={colors.accent} />
                ) : null
              }
              onPress={() => pick(name)}
            />
          </Fragment>
        ))}
      </View>
    </>
  );
}

/** Tool calls folded to their phase's label until tapped, on by default. */
function TranscriptSection() {
  const colors = useColors();
  const styles = useStyles();
  const [collapsed, setCollapsed] = useCollapsedToolCalls();
  return (
    <>
      <Text style={styles.heading}>Transcript</Text>
      <View style={styles.card}>
        <SettingsRow
          icon="wrench"
          label="Collapse tool calls"
          onPress={() => setCollapsed(!collapsed)}
          accessory={
            <Switch
              value={collapsed}
              onValueChange={setCollapsed}
              trackColor={{ true: colors.accent }}
            />
          }
        />
      </View>
      <Text style={styles.note}>
        Show only the label of the step the agent is on. Tap a step to see its
        tool calls.
      </Text>
    </>
  );
}

/** A theme in both schemes at once: its light and dark backgrounds split
 *  across a circle, its accent at the centre. */
function Swatch({ name }: { name: ThemeName }) {
  const styles = useStyles();
  const light = paletteFor("light", name);
  const dark = paletteFor("dark", name);
  return (
    <View style={styles.swatch}>
      <View style={[styles.swatchHalf, { backgroundColor: light.surface }]} />
      <View style={[styles.swatchHalf, { backgroundColor: dark.surface }]} />
      <View style={[styles.swatchAccent, { backgroundColor: dark.accent }]} />
    </View>
  );
}

/** The project's empty-session background: the photo as the session shows
 *  it, picked, changed or removed here, and how strongly it shows. */
function BackgroundSection({ projectId }: { projectId: string }) {
  const colors = useColors();
  const styles = useStyles();
  const controls = useEmptyBackground(projectId);
  const { settings, background, busy } = controls;
  return (
    <View style={[styles.card, styles.backgroundCard]}>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={settings.image ? "Change photo" : "Choose photo"}
        onPress={() => void controls.pickImage()}
        style={[styles.preview, { backgroundColor: colors.background }]}
      >
        {background ? (
          <Image
            source={background.source}
            resizeMode="cover"
            style={[StyleSheet.absoluteFill, { opacity: background.opacity }]}
          />
        ) : null}
        {busy ? (
          <ActivityIndicator color={colors.muted} />
        ) : settings.image ? null : (
          <View style={styles.previewEmpty}>
            <Icon name="image" size={26} color={colors.muted} />
            <Text style={styles.previewHint}>Choose a photo</Text>
          </View>
        )}
      </Pressable>

      <View style={styles.backgroundRows}>
        {settings.image ? (
          <>
            <View style={styles.visibility}>
              <Host style={styles.slider}>
                <Slider
                  value={settings.opacity}
                  min={OPACITY_RANGE.min}
                  max={OPACITY_RANGE.max}
                  step={0.01}
                  onValueChange={controls.setOpacity}
                  modifiers={[
                    tint(colors.accent),
                    accessibilityLabel("Background visibility"),
                  ]}
                />
              </Host>
            </View>
            <View style={styles.divider} />
          </>
        ) : null}
        <SettingsRow
          icon="image"
          label={settings.image ? "Change photo" : "Choose photo"}
          onPress={() => void controls.pickImage()}
        />
        {settings.image ? (
          <>
            <View style={styles.divider} />
            <SettingsRow
              icon="trash"
              label="Remove photo"
              showChevron={false}
              destructive
              onPress={controls.removeImage}
            />
          </>
        ) : null}
      </View>
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  heading: {
    color: colors.muted,
    fontSize: 15,
    marginTop: 24,
    marginBottom: 8,
    marginLeft: 16,
  },
  card: {
    backgroundColor: colors.surface,
    borderRadius: radii.card,
    borderCurve: "continuous",
    paddingHorizontal: 10,
    paddingVertical: 6,
  },
  // Inset to the rows' labels, past their icons.
  divider: {
    height: StyleSheet.hairlineWidth,
    marginLeft: 40,
    backgroundColor: colors.stroke,
  },
  note: {
    color: colors.muted,
    fontSize: 13,
    lineHeight: 18,
    marginTop: 8,
    marginHorizontal: 16,
  },
  firstHeading: {
    marginTop: 4,
  },
  pressed: {
    opacity: 0.7,
  },
  backgroundCard: {
    paddingVertical: 12,
    paddingHorizontal: 12,
    gap: 4,
  },
  preview: {
    height: 220,
    borderRadius: radii.card - 8,
    overflow: "hidden",
    alignItems: "center",
    justifyContent: "center",
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: colors.stroke,
  },
  previewEmpty: {
    alignItems: "center",
    gap: 8,
  },
  previewHint: {
    color: colors.muted,
    fontSize: 15,
  },
  // Under the preview, in line with the other cards' rows.
  backgroundRows: {
    marginTop: 6,
    marginHorizontal: -2,
  },
  swatch: {
    width: 24,
    height: 24,
    borderRadius: 12,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
    overflow: "hidden",
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: colors.stroke,
  },
  swatchHalf: {
    flex: 1,
    alignSelf: "stretch",
  },
  swatchAccent: {
    position: "absolute",
    width: 10,
    height: 10,
    borderRadius: 5,
  },
  visibility: {
    paddingVertical: 4,
    paddingHorizontal: 6,
  },
  slider: {
    height: 44,
  },
}));

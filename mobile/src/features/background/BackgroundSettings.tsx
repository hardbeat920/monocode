import { semantics } from "@expo/ui/jetpack-compose/modifiers";
import { Host, Slider } from "@expo/ui";
import { accessibilityLabel, tint } from "@expo/ui/swift-ui/modifiers";
import {
  ActivityIndicator,
  Image,
  Platform,
  Pressable,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { EFFECTS, EFFECT_LABELS } from "./settings";
import { OPACITY_RANGE, useEmptyBackground } from "./useEmptyBackground";
import { SettingsRow } from "@/features/settings/SettingsUI";
import { Icon } from "@/ui/Icon";
import { radii, themedStyles, useColors } from "@/ui/theme";
/** The project's empty-session background: the photo as the session shows
 *  it, picked, changed or removed here, and how strongly it shows. */
export function BackgroundSection({ projectId }: { projectId: string }) {
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
            <Text style={styles.heading}>Background effect</Text>
            <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 8 }}>
              {EFFECTS.map((effect) => (
                <Pressable
                  key={effect}
                  accessibilityRole="button"
                  accessibilityState={{ selected: settings.effect === effect }}
                  onPress={() => controls.update({ effect })}
                  style={{
                    padding: 12,
                    borderRadius: 10,
                    borderWidth: 1,
                    borderColor:
                      settings.effect === effect
                        ? colors.accent
                        : colors.stroke,
                  }}
                >
                  <Text
                    style={{
                      color:
                        settings.effect === effect
                          ? colors.accent
                          : colors.content,
                    }}
                  >
                    {EFFECT_LABELS[effect]}
                  </Text>
                </Pressable>
              ))}
            </View>
            <Text style={styles.heading}>Show on</Text>
            <View style={{ flexDirection: "row", gap: 8 }}>
              {(["empty", "all"] as const).map((scope) => (
                <Pressable
                  key={scope}
                  accessibilityRole="button"
                  accessibilityState={{ selected: settings.scope === scope }}
                  onPress={() => controls.update({ scope })}
                  style={{
                    padding: 12,
                    borderRadius: 10,
                    borderWidth: 1,
                    borderColor:
                      settings.scope === scope ? colors.accent : colors.stroke,
                  }}
                >
                  <Text
                    style={{
                      color:
                        settings.scope === scope
                          ? colors.accent
                          : colors.content,
                    }}
                  >
                    {scope === "empty" ? "Empty sessions" : "All sessions"}
                  </Text>
                </Pressable>
              ))}
            </View>
            <View style={styles.visibilityControls}>
              <VisibilityControl
                label="Empty chat visibility"
                value={settings.opacity}
                onChange={controls.setOpacity}
              />
              <VisibilityControl
                label="Session visibility"
                value={settings.sessionOpacity}
                onChange={(sessionOpacity) => controls.update({ sessionOpacity })}
              />
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

function VisibilityControl({
  label,
  value,
  onChange,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  return (
    <View
      style={styles.visibility}
      accessibilityLabel={label}
      accessibilityValue={{ min: 5, max: 65, now: Math.round(value * 100) }}
    >
      <Text style={{ color: colors.content }}>
        {label} · {Math.round(value * 100)}%
      </Text>
      <Host style={styles.slider}>
        <Slider
          value={value}
          min={OPACITY_RANGE.min}
          max={OPACITY_RANGE.max}
          step={0.01}
          onValueChange={onChange}
          modifiers={
            Platform.OS === "ios"
              ? [tint(colors.accent), accessibilityLabel(label)]
              : [semantics({ contentDescription: label })]
          }
        />
      </Host>
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  heading: {
    color: colors.muted,
    fontSize: 15,
    marginTop: 24,
    marginBottom: 8,
    // SettingsRow centers an 18pt icon inside a 36pt slot.
    marginLeft: 9,
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
  visibilityControls: {
    marginTop: 16,
  },
  visibility: {
    paddingVertical: 4,
    paddingHorizontal: 6,
  },
  slider: {
    height: 44,
  },
}));

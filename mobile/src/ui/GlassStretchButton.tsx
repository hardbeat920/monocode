import { Button, Host, Image } from "@expo/ui/swift-ui";
import {
  accessibilityLabel,
  animation,
  Animation,
  buttonBorderShape,
  buttonStyle,
  disabled as disabledModifier,
  frame,
  opacity,
} from "@expo/ui/swift-ui/modifiers";
import { isLiquidGlassAvailable } from "expo-glass-effect";
import type { ComponentProps } from "react";
import { StyleSheet, View } from "react-native";

import { GLASS_BUTTON_PADDING } from "./GlassMenuButton";
import { Icon, type IconName } from "./Icon";
import { LiquidView } from "./LiquidView";
import { themedStyles, useColors } from "./theme";

const GLYPH_FADE = 0.16;
/**
 * What SwiftUI's capsule glass button adds beside its label, both sides
 * together, measured on the simulator; more than it adds around a circle's.
 * A label this much narrower than the button leaves it round at rest, and
 * keeps the glyph centred on the round end however far it's drawn out.
 */
const CAPSULE_SIDES = 24;
/** Wider than any screen: the label fills what the Host gives it. */
const FILL = 10000;

/**
 * A round glass button that fills its parent, so a parent that widens draws it
 * out into a capsule, the glyph staying at the trailing end. On iOS 26 it is
 * SwiftUI's glass button, the same material as GlassMenuButton: the app's own
 * GlassView comes out a visibly different grey beside one.
 *
 * The Host ignores the keyboard's safe area, for the reason GlassMenuButton's do.
 */
export function GlassStretchButton({
  icon,
  symbol,
  label,
  size,
  glyphShown,
  disabled,
  onPress,
}: {
  /** The icon where the glass is the app's own. */
  icon: IconName;
  /** The SF Symbol where the button is SwiftUI's. */
  symbol: ComponentProps<typeof Image>["systemName"];
  label: string;
  size: number;
  glyphShown: boolean;
  disabled: boolean;
  onPress: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();

  if (isLiquidGlassAvailable()) {
    return (
      <Host ignoreSafeArea="keyboard" style={StyleSheet.absoluteFill}>
        <Button
          onPress={onPress}
          modifiers={[
            buttonStyle("glass"),
            buttonBorderShape("capsule"),
            disabledModifier(disabled),
            accessibilityLabel(label),
          ]}
        >
          <Image
            systemName={symbol}
            size={18}
            color={colors.content}
            modifiers={[
              opacity(glyphShown ? 1 : 0),
              animation(Animation.easeOut({ duration: GLYPH_FADE }), glyphShown),
              frame({ width: size - CAPSULE_SIDES, height: size - GLASS_BUTTON_PADDING }),
              frame({ maxWidth: FILL, alignment: "trailing" }),
            ]}
          />
        </Button>
      </Host>
    );
  }
  return (
    <LiquidView
      fill
      haptics={false}
      disabled={disabled}
      accessibilityLabel={label}
      borderRadius={size / 2}
      onPress={onPress}
      style={{ height: size }}
    >
      <View style={[styles.end, { width: size }, disabled && styles.disabled]}>
        {glyphShown ? <Icon name={icon} size={18} color={colors.content} /> : null}
      </View>
    </LiquidView>
  );
}

const useStyles = themedStyles(() => ({
  end: {
    position: "absolute",
    top: 0,
    right: 0,
    bottom: 0,
    alignItems: "center",
    justifyContent: "center",
  },
  disabled: {
    opacity: 0.4,
  },
}));

import { Host, HStack, Image, Menu, Spacer } from "@expo/ui/swift-ui";
import {
  accessibilityLabel,
  buttonBorderShape,
  buttonStyle,
  contentShape,
  frame,
  menuIndicator,
  shapes,
} from "@expo/ui/swift-ui/modifiers";
import { isLiquidGlassAvailable } from "expo-glass-effect";
import type { ComponentProps, ReactNode } from "react";
import { StyleSheet, View } from "react-native";

import { Icon, type IconName } from "./Icon";
import { LiquidView } from "./LiquidView";
import { useColors } from "./theme";

/** What SwiftUI's glass button adds around its label, both sides together, measured on the simulator. */
const PADDING = 14;

/**
 * A round glass button that opens a native menu from itself, the menu
 * growing out of the button and folding back into it as the system's own
 * menus do. On iOS 26 the button is SwiftUI's glass button with the menu
 * inside, which is what lets it morph; earlier, the app's glass with the menu
 * laid over it. `children` are the menu's items; `on` tells a screen reader
 * that a filter is in effect, and the icon stays its usual colour.
 *
 * The Hosts ignore the keyboard's safe area. A composer rides the keyboard on a
 * layer transform, which UIKit doesn't see, so SwiftUI goes on measuring the
 * button at its resting place, behind the keyboard, and pushes it half its
 * height out of the frame until something re-lays it out.
 */
export function GlassMenuButton({
  icon,
  symbol,
  label,
  size,
  on = false,
  children,
}: {
  /** The icon where the glass is the app's own. */
  icon: IconName;
  /** The SF Symbol where the button is SwiftUI's. */
  symbol: ComponentProps<typeof Image>["systemName"];
  /** What a screen reader says; ", on" is added while `on`. */
  label: string;
  size: number;
  on?: boolean;
  children: ReactNode;
}) {
  const colors = useColors();
  const spoken = on ? `${label}, on` : label;

  if (isLiquidGlassAvailable()) {
    return (
      <View style={{ width: size, height: size }}>
        <Host ignoreSafeArea="keyboard" style={{ width: size, height: size }}>
          <Menu
            modifiers={[
              buttonStyle("glass"),
              buttonBorderShape("circle"),
              menuIndicator("hidden"),
              accessibilityLabel(spoken),
            ]}
            label={
              <Image
                systemName={symbol}
                size={18}
                color={colors.content}
                modifiers={[frame({ width: size - PADDING, height: size - PADDING })]}
              />
            }
          >
            {children}
          </Menu>
        </Host>
      </View>
    );
  }
  return (
    <View>
      <LiquidView borderRadius={size / 2} innerStyle={{ width: size, height: size }}>
        <Icon name={icon} size={20} color={colors.content} />
      </LiquidView>
      <Host ignoreSafeArea="keyboard" style={StyleSheet.absoluteFill}>
        <Menu
          modifiers={[accessibilityLabel(spoken)]}
          label={
            <HStack
              modifiers={[frame({ width: size, height: size }), contentShape(shapes.circle())]}
            >
              <Spacer />
            </HStack>
          }
        >
          {children}
        </Menu>
      </Host>
    </View>
  );
}

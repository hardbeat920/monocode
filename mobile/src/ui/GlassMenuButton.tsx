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
 * laid over it. `children` are the menu's items; `on` lights the icon, for a
 * filter that's in effect.
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
  /** What a screen reader says; ", on" is added while lit. */
  label: string;
  size: number;
  on?: boolean;
  children: ReactNode;
}) {
  const colors = useColors();
  const spoken = on ? `${label}, on` : label;
  const tint = on ? colors.accent : colors.content;

  if (isLiquidGlassAvailable()) {
    return (
      <View style={{ width: size, height: size }}>
        <Host style={{ width: size, height: size }}>
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
                color={tint}
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
        <Icon name={icon} size={20} color={tint} />
      </LiquidView>
      <Host style={StyleSheet.absoluteFill}>
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

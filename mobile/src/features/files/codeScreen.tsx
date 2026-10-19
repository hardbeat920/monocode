import { useState, type ReactNode } from "react";
import type { NativeStackNavigationOptions } from "expo-router";
import { useHeaderHeight } from "expo-router/react-navigation";
import { Platform, Pressable, Text, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { GradualBlur } from "@/ui/GradualBlur";
import { Icon, type IconName } from "@/ui/Icon";
import { themedStyles, useColors, type Palette } from "@/ui/theme";

/** iOS 26 and later draw their own scroll edge effect under bars. */
const SCROLL_EDGE_EFFECTS =
  Platform.OS === "ios" && Number.parseInt(String(Platform.Version), 10) >= 26;

/** How far a header's blur reaches past its bottom, fading out. */
const HEADER_FADE = 24;

/** The bottom blur's reach above the safe area, over a search bar there. */
const SEARCH_BAR_REACH = 44 + HEADER_FADE;

/**
 * No blur from iOS, for a transparent native header over a screen that draws
 * its own gradual one under it (see SessionFrame and HeaderBlur): neither the
 * header's material, which ends in a hard edge, nor iOS's edge effects, which
 * since iOS 27 no longer fade out gradually.
 */
export const HEADER_OWN_BLUR: NativeStackNavigationOptions = SCROLL_EDGE_EFFECTS
  ? { scrollEdgeEffects: { top: "hidden", bottom: "hidden" } }
  : {};

/**
 * The gradual blur under a transparent native header, reaching a little past
 * it. Render it after the screen's content.
 */
export function HeaderBlur({ color }: { color: string }) {
  const height = useHeaderHeight();
  return <GradualBlur edge="top" height={height + HEADER_FADE} color={color} />;
}

/**
 * Each screen of a modal's own stack: its content, then the header's blur
 * and, where iOS puts the header's search bar at the bottom, one under it.
 */
export function sheetScreenLayout(color: string) {
  return function SheetScreenLayout({
    children,
    options,
  }: {
    children: ReactNode;
    options: NativeStackNavigationOptions;
  }) {
    return (
      <SheetScreen color={color} searchBar={!!options.headerSearchBarOptions}>
        {children}
      </SheetScreen>
    );
  };
}

function SheetScreen({
  color,
  searchBar,
  children,
}: {
  color: string;
  searchBar: boolean;
  children: ReactNode;
}) {
  return (
    <>
      {children}
      <HeaderBlur color={color} />
      {SCROLL_EDGE_EFFECTS && searchBar ? <SearchBarBlur color={color} /> : null}
    </>
  );
}

function SearchBarBlur({ color }: { color: string }) {
  const { bottom } = useSafeAreaInsets();
  return (
    <GradualBlur edge="bottom" height={bottom + SEARCH_BAR_REACH} color={color} />
  );
}

/**
 * The file and diff screens: native modals with a transparent native header
 * that the code scrolls under, blurred as it goes by a HeaderBlur of their own. Their content pads itself
 * below the header through iOS's automatic
 * insets, so lists there set `contentInsetAdjustmentBehavior="automatic"`.
 */
export const codeScreenOptions = (colors: Palette): NativeStackNavigationOptions => ({
  presentation: "modal",
  // The stack swaps its other screens without animating; these slide up as
  // modals do.
  animation: "default",
  headerShown: true,
  headerTransparent: true,
  headerShadowVisible: false,
  headerTintColor: colors.content,
  headerBackVisible: false,
  ...HEADER_OWN_BLUR,
  contentStyle: { backgroundColor: colors.sheet },
});

/**
 * The screens of a modal with a stack of its own, as settings and the project
 * picker are: a transparent native header over the sheet's colour, which the
 * stack would otherwise cover in the page's darker one. Pair it with
 * `sheetScreenLayout` for the header's blur.
 */
export const modalStackOptions = (colors: Palette): NativeStackNavigationOptions => ({
  headerShown: true,
  headerTransparent: true,
  headerShadowVisible: false,
  headerTintColor: colors.content,
  headerBackButtonDisplayMode: "minimal",
  headerTitleStyle: { fontWeight: "600" },
  ...HEADER_OWN_BLUR,
  contentStyle: { backgroundColor: colors.sheet },
});

// Shared by the file and diff screens, and remembered between them for as
// long as the app runs.
let wrapPreference = false;

/** Whether code screens wrap long lines, and a toggle that remembers it. */
export function useWrapPreference() {
  const [wrap, setWrap] = useState(wrapPreference);
  return [wrap, () => setWrap((wrapPreference = !wrap))] as const;
}

/** The header's title: the file's name over a line of details. */
export function CodeHeaderTitle({
  name,
  children,
}: {
  name: string;
  children: ReactNode;
}) {
  const styles = useStyles();
  return (
    <View style={styles.title}>
      <Text style={styles.name} numberOfLines={1}>
        {name}
      </Text>
      <Text style={styles.meta} numberOfLines={1}>
        {children}
      </Text>
    </View>
  );
}

/** An icon button for the native header; `on` turns it the accent, for a toggle. */
export function HeaderButton({
  icon,
  label,
  onPress,
  on,
}: {
  icon: IconName;
  label: string;
  onPress: () => void;
  on?: boolean;
}) {
  const colors = useColors();
  const styles = useStyles();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={on === undefined ? undefined : { selected: on }}
      hitSlop={8}
      onPress={onPress}
      style={styles.button}
    >
      <Icon name={icon} size={20} color={on ? colors.accent : colors.content} />
    </Pressable>
  );
}

/** Header buttons side by side. */
export function HeaderButtons({ children }: { children: ReactNode }) {
  const styles = useStyles();
  return <View style={styles.buttons}>{children}</View>;
}

const useStyles = themedStyles((colors) => ({
  title: {
    alignItems: "center",
    maxWidth: 220,
  },
  name: {
    color: colors.content,
    fontSize: 16,
    fontWeight: "600",
  },
  meta: {
    color: colors.muted,
    fontSize: 12,
    fontVariant: ["tabular-nums"],
  },
  buttons: {
    flexDirection: "row",
    alignItems: "center",
    gap: 4,
  },
  button: {
    width: 36,
    height: 36,
    alignItems: "center",
    justifyContent: "center",
  },
}));

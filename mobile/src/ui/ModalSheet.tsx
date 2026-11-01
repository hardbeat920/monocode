import type { NativeStackNavigationOptions } from "expo-router";
import type { ReactNode } from "react";
import { Platform, Pressable, View } from "react-native";
import Animated, { FadeIn, SlideInDown } from "react-native-reanimated";
import { SafeAreaInsetsContext, useSafeAreaInsets } from "react-native-safe-area-context";

import { themedStyles } from "./theme";

/**
 * The root stack's options for a modal with its own stack, laid out by
 * ModalSheet. iOS presents it as a native page sheet; Android, whose modals
 * are full-screen pages, leaves the screen under it showing for ModalSheet
 * to draw the sheet over it.
 */
export function modalSheetOptions(sheet: string): NativeStackNavigationOptions {
  return Platform.OS === "android"
    ? {
        presentation: "transparentModal",
        animation: "none",
        contentStyle: { backgroundColor: "transparent" },
      }
    : {
        presentation: "modal",
        animation: "default",
        contentStyle: { backgroundColor: sheet },
      };
}

/**
 * A modal's own stack in a sheet with a grabber. On iOS the native sheet
 * holds it and the grabber is drawn over its header. On Android it is the
 * Workspace sheet's design, raised over the dimmed screen below: round top
 * corners, a handle, and a tap above it to close.
 */
export function ModalSheet({
  onClose,
  children,
}: {
  onClose: () => void;
  children: ReactNode;
}) {
  const styles = useStyles();
  const insets = useSafeAreaInsets();
  if (Platform.OS !== "android")
    return (
      <View style={styles.fill}>
        {children}
        <View pointerEvents="none" style={styles.grabber} />
      </View>
    );
  return (
    <View style={styles.fill}>
      <Animated.View entering={FadeIn.duration(180)} style={styles.dim}>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Close"
          onPress={onClose}
          style={styles.fill}
        />
      </Animated.View>
      <Animated.View
        entering={SlideInDown.duration(280)}
        accessibilityViewIsModal
        style={[styles.sheet, { marginTop: insets.top + 16 }]}
      >
        {/* The sheet starts below the status bar: its stack's header, and
            anything else inside, take no top inset of their own. */}
        <SafeAreaInsetsContext.Provider value={{ ...insets, top: 0 }}>
          {children}
        </SafeAreaInsetsContext.Provider>
        <View pointerEvents="none" style={styles.handle} />
      </Animated.View>
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  fill: { flex: 1 },
  // iOS's own sheet grabber.
  grabber: {
    position: "absolute",
    top: 5,
    alignSelf: "center",
    width: 36,
    height: 5,
    borderRadius: 2.5,
    backgroundColor: colors.faint,
  },
  dim: {
    position: "absolute",
    top: 0,
    right: 0,
    bottom: 0,
    left: 0,
    backgroundColor: "rgba(0,0,0,0.4)",
  },
  // As WorkspaceMenu.android's sheet.
  sheet: {
    flex: 1,
    backgroundColor: colors.sheet,
    borderTopLeftRadius: 32,
    borderTopRightRadius: 32,
    overflow: "hidden",
  },
  handle: {
    position: "absolute",
    top: 8,
    alignSelf: "center",
    width: 40,
    height: 4,
    borderRadius: 2,
    backgroundColor: colors.faint,
  },
}));

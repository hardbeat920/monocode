import { frame, ignoreSafeArea } from "@expo/ui/swift-ui/modifiers";
import MaskedView from "@react-native-masked-view/masked-view";
import { LinearGradient } from "expo-linear-gradient";
import type { ReactNode } from "react";
import { Platform, StyleSheet, View, type StyleProp, type ViewStyle } from "react-native";

/** How far a list fades out where it meets the title and the sheet's foot. */
export const SHEET_FADE = 18;

const IN = ["transparent", "black"] as const;
const OUT = ["black", "transparent"] as const;

/**
 * For a BottomSheet with snap points whose content scrolls: the content takes
 * the sheet's real height, right down to its bottom edge. iOS makes a sheet
 * taller than asked and keeps content out of the home indicator's strip, where
 * a list would otherwise stop short of the edge in a hard line.
 */
export const SHEET_FILL =
  Platform.OS === "ios"
    ? [
        frame({ maxWidth: Infinity, maxHeight: Infinity, alignment: "topLeading" }),
        ignoreSafeArea({ regions: "container", edges: "bottom" }),
      ]
    : undefined;

/**
 * Content that fills the sheet at whatever height it rests. It lies over the
 * host rather than in it: in the host, a long list would stretch it past the
 * sheet, and the end of the list could never be reached.
 */
export function SheetBody({
  children,
  width,
  style,
}: {
  children: ReactNode;
  width: number;
  style?: StyleProp<ViewStyle>;
}) {
  return (
    <View style={{ flex: 1, width }}>
      <View style={[StyleSheet.absoluteFill, style]}>{children}</View>
    </View>
  );
}

/**
 * Fades a scroll view out at both ends. Its content needs SHEET_FADE of padding
 * at each end, so that nothing is faded at rest.
 */
export function SheetFade({ children }: { children: ReactNode }) {
  return (
    <MaskedView
      style={styles.fill}
      maskElement={
        <View style={styles.fill}>
          <LinearGradient colors={IN} style={styles.edge} />
          <View style={styles.solid} />
          <LinearGradient colors={OUT} style={styles.edge} />
        </View>
      }
    >
      {children}
    </MaskedView>
  );
}

const styles = StyleSheet.create({
  fill: { flex: 1 },
  edge: { height: SHEET_FADE },
  solid: { flex: 1, backgroundColor: "black" },
});

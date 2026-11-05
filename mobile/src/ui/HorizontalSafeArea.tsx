import type { ReactNode } from "react";
import { StyleSheet, View } from "react-native";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";

/** Keep page content clear of side controls, including Duo's folded navigation
 * bar. Measure inside the native screen or sheet, whose safe area can differ
 * from the window's. Leave vertical spacing to the page and its header. */
export function HorizontalSafeArea({ children }: { children: ReactNode }) {
  return (
    <SafeAreaProvider style={styles.fill}>
      <SafeAreaView edges={["left", "right"]} style={styles.fill}>
        {/* Absolute controls (search/filter bars) must anchor to the narrowed
            content frame, rather than the padded safe-area view's edges. */}
        <View style={styles.fill}>{children}</View>
      </SafeAreaView>
    </SafeAreaProvider>
  );
}

const styles = StyleSheet.create({
  fill: { flex: 1 },
});

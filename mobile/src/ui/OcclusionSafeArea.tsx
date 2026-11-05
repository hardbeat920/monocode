import { requireNativeView } from "expo";
import { useState, type ReactNode } from "react";
import { StyleSheet, View, type ViewProps } from "react-native";
import { SafeAreaInsetsContext, useSafeAreaInsets } from "react-native-safe-area-context";

import { hasOcclusionObserver } from "@/features/connection/nativeLink";
import { occlusionInsets, type OcclusionMeasurement } from "./occlusionInsets";

type ObserverProps = ViewProps & {
  onOcclusionChange: (event: { nativeEvent: OcclusionMeasurement }) => void;
};
const Observer = hasOcclusionObserver
  ? requireNativeView<ObserverProps>("MonocodeLink", "OcclusionObserverView")
  : null;

/** A floating control may use clear space beside the rail on iOS 27.1+.
 * Older native builds keep the enclosing screen's horizontal safe area. */
export function OcclusionSafeArea({ children, fill = false, margin = 0, insetMargin = 0 }: {
  children: ReactNode;
  /** Fill a group whose animated height is owned by its parent. */
  fill?: boolean;
  /** Normal edge margin, replaced by insetMargin on an obstructed edge. */
  margin?: number;
  insetMargin?: number;
}) {
  const safe = useSafeAreaInsets();
  const [measurement, setMeasurement] = useState<OcclusionMeasurement | null>(null);
  const occupied = measurement?.supported ? occlusionInsets(measurement) : safe;
  const insets = {
    ...safe,
    // While avoiding the rail, use the toolbar's safe edge. When clear, let
    // the control use the normal screen margin instead of the entire strip.
    left: occupied.left > 0 ? Math.max(safe.left, occupied.left) : 0,
    right: occupied.right > 0 ? Math.max(safe.right, occupied.right) : 0,
  };
  return (
    <View pointerEvents="box-none" style={fill ? styles.fill : undefined}>
      {Observer ? <Observer
        pointerEvents="none"
        accessible={false}
        style={StyleSheet.absoluteFill}
        onOcclusionChange={({ nativeEvent }) => setMeasurement(nativeEvent)}
      /> : null}
      <SafeAreaInsetsContext value={insets}>
        <View pointerEvents="box-none" style={[fill && styles.fill, {
          paddingLeft: insets.left + (insets.left > 0 ? insetMargin : margin),
          paddingRight: insets.right + (insets.right > 0 ? insetMargin : margin),
        }]}>
          {/* Absolute cards anchor to this narrowed frame, not the padding box. */}
          <View pointerEvents="box-none" style={fill ? styles.fill : undefined}>{children}</View>
        </View>
      </SafeAreaInsetsContext>
    </View>
  );
}

const styles = StyleSheet.create({ fill: { flex: 1 } });

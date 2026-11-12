import type { ReactNode } from "react";
import { StyleSheet, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

/** Give floating controls consistent horizontal clearance beside system bars.
 * Keep the same width as the keyboard moves, rather than expanding after it
 * settles. The enclosing screen/provider supplies the local safe area. */
export function FloatingSafeArea({ children, fill = false, margin = 0, insetMargin = 0 }: {
  children: ReactNode;
  /** Fill a group whose animated height is owned by its parent. */
  fill?: boolean;
  /** Normal edge margin, replaced by insetMargin beside a system bar. */
  margin?: number;
  insetMargin?: number;
}) {
  const insets = useSafeAreaInsets();
  return (
    <View pointerEvents="box-none" style={[fill && styles.fill, {
      paddingLeft: insets.left + (insets.left > 0 ? insetMargin : margin),
      paddingRight: insets.right + (insets.right > 0 ? insetMargin : margin),
    }]}>
      {/* Absolute cards anchor to this narrowed frame, not the padding box. */}
      <View pointerEvents="box-none" style={fill ? styles.fill : undefined}>{children}</View>
    </View>
  );
}

const styles = StyleSheet.create({ fill: { flex: 1 } });

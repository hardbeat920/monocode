import * as Haptics from "expo-haptics";
import { Pressable, Text } from "react-native";
import Animated, {
  FadeInDown,
  FadeOutDown,
  ReduceMotion,
} from "react-native-reanimated";

import { themedStyles } from "@/ui/theme";
import type { ToastState } from "@/ui/useToast";

/**
 * The line under a list that says what a change did, with Undo while it can
 * still be taken back, else OK. Placed `bottom` points above the screen's
 * foot.
 */
export function UndoToast({
  toast,
  onDismiss,
  bottom,
}: {
  toast: ToastState | null;
  onDismiss: () => void;
  bottom: number;
}) {
  const styles = useStyles();
  if (!toast) return null;
  return (
    <Animated.View
      key={toast.id}
      accessibilityLiveRegion="polite"
      entering={FadeInDown.duration(220).reduceMotion(ReduceMotion.System)}
      exiting={FadeOutDown.duration(160).reduceMotion(ReduceMotion.System)}
      style={[styles.toast, { bottom }]}
    >
      <Text numberOfLines={1} style={styles.message}>
        {toast.message}
      </Text>
      {toast.undo ? (
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Undo"
          onPress={() => {
            void Haptics.selectionAsync();
            toast.undo?.();
          }}
          style={({ pressed }) => [styles.undo, pressed && styles.pressed]}
        >
          <Text style={styles.undoLabel}>Undo</Text>
        </Pressable>
      ) : (
        <Pressable accessibilityRole="button" accessibilityLabel="Dismiss" onPress={onDismiss}>
          <Text style={styles.undoLabel}>OK</Text>
        </Pressable>
      )}
    </Animated.View>
  );
}

const useStyles = themedStyles((colors) => ({
  toast: {
    position: "absolute",
    left: 16,
    right: 16,
    height: 48,
    borderRadius: 24,
    borderCurve: "continuous",
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    paddingLeft: 18,
    paddingRight: 8,
    backgroundColor: colors.inverseSurface,
    zIndex: 60,
  },
  message: {
    flex: 1,
    color: colors.inverse,
    fontSize: 15,
    fontWeight: "500",
  },
  undo: {
    height: 34,
    justifyContent: "center",
    paddingHorizontal: 14,
    borderRadius: 17,
    backgroundColor: "rgba(128, 128, 128, 0.25)",
  },
  pressed: {
    opacity: 0.6,
  },
  undoLabel: {
    color: colors.inverse,
    fontSize: 14,
    fontWeight: "700",
  },
}));

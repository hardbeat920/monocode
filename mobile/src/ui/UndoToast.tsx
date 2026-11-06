import * as Haptics from "expo-haptics";
import { useState } from "react";
import { Pressable, Text } from "react-native";
import Animated from "react-native-reanimated";

import {
  GlassPresence,
  LiquidView,
  useGlassEntrance,
  usePopStyle,
} from "@/ui/LiquidView";
import { themedStyles } from "@/ui/theme";
import type { ToastState } from "@/ui/useToast";

const PRESENCE_MS = 220;

/**
 * A glass capsule under a list that says what a change did, with Undo while it
 * can still be taken back. Placed `bottom` points above the screen's foot; a
 * tap on one with no Undo sends it away early.
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
  // Kept while it drops away, so its words don't go first.
  const [held, setHeld] = useState(toast);
  if (toast && toast !== held) setHeld(toast);
  const { mounted, visible, presence } = useGlassEntrance(!!toast, PRESENCE_MS);
  const pop = usePopStyle(visible, PRESENCE_MS);
  if (!mounted || !held) return null;
  return (
    <GlassPresence value={presence}>
      <Animated.View
        accessibilityLiveRegion="polite"
        pointerEvents={visible ? "box-none" : "none"}
        style={[styles.place, { bottom }, pop]}
      >
        <LiquidView
          onPress={held.undo ? undefined : onDismiss}
          accessibilityLabel={held.message}
          haptics={false}
          borderRadius={22}
          innerStyle={[styles.capsule, held.undo && styles.capsuleWithUndo]}
        >
          <Text numberOfLines={2} style={styles.message}>
            {held.message}
          </Text>
          {held.undo ? (
            <Pressable
              accessibilityRole="button"
              accessibilityLabel="Undo"
              hitSlop={6}
              onPress={() => {
                void Haptics.selectionAsync();
                held.undo?.();
              }}
              style={({ pressed }) => [styles.undo, pressed && styles.pressed]}
            >
              <Text style={styles.undoLabel}>Undo</Text>
            </Pressable>
          ) : null}
        </LiquidView>
      </Animated.View>
    </GlassPresence>
  );
}

const useStyles = themedStyles((colors) => ({
  place: {
    position: "absolute",
    left: 16,
    right: 16,
    alignItems: "center",
    zIndex: 60,
  },
  capsule: {
    minHeight: 44,
    paddingVertical: 10,
    paddingHorizontal: 18,
    gap: 4,
  },
  capsuleWithUndo: {
    paddingVertical: 6,
    paddingRight: 6,
  },
  message: {
    flexShrink: 1,
    color: colors.content,
    fontSize: 15,
    fontWeight: "500",
  },
  undo: {
    height: 32,
    justifyContent: "center",
    paddingHorizontal: 12,
  },
  pressed: {
    opacity: 0.6,
  },
  undoLabel: {
    color: colors.accent,
    fontSize: 15,
    fontWeight: "600",
  },
}));

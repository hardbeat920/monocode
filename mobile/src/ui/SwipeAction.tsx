import { Pressable, View } from "react-native";
import Animated, {
  interpolate,
  useAnimatedStyle,
  type SharedValue,
} from "react-native-reanimated";

import { Icon, type IconName } from "@/ui/Icon";
import { radii, themedStyles } from "@/ui/theme";

/** A swipe action's button and the gap around the buttons. */
export const ACTION_SIZE = 40;
export const ACTION_GAP = 8;

/**
 * The buttons uncovered by swiping a row left, laid out as in Mail. Each
 * button is a `SwipeAction`, nearest the edge first to appear.
 */
export function SwipeActions({ children }: { children: React.ReactNode }) {
  const styles = useStyles();
  return <View style={styles.actions}>{children}</View>;
}

/** A button's face: it fades and grows in once the row is `from` of the way open. */
export function ActionFace({
  progress,
  from,
  icon,
  color,
  background,
}: {
  progress: SharedValue<number>;
  /** How far open the row is when this button starts to appear. */
  from: number;
  icon: IconName;
  color: string;
  background: string;
}) {
  const styles = useStyles();
  const style = useAnimatedStyle(() => {
    const shown = interpolate(progress.get(), [from, from + 0.65], [0, 1], "clamp");
    return { opacity: shown, transform: [{ scale: 0.7 + 0.3 * shown }] };
  });
  return (
    <Animated.View style={[styles.action, { backgroundColor: background }, style]}>
      <Icon name={icon} size={18} color={color} />
    </Animated.View>
  );
}

export function SwipeAction({
  progress,
  from,
  icon,
  label,
  color,
  background,
  onPress,
}: {
  progress: SharedValue<number>;
  from: number;
  icon: IconName;
  label: string;
  color: string;
  background: string;
  onPress: () => void;
}) {
  return (
    <Pressable accessibilityRole="button" accessibilityLabel={label} onPress={onPress}>
      <ActionFace
        progress={progress}
        from={from}
        icon={icon}
        color={color}
        background={background}
      />
    </Pressable>
  );
}

const useStyles = themedStyles(() => ({
  actions: {
    flexDirection: "row",
    alignItems: "center",
    gap: ACTION_GAP,
    paddingLeft: ACTION_GAP,
    paddingRight: 4,
  },
  action: {
    width: ACTION_SIZE,
    height: ACTION_SIZE,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: radii.row,
    borderCurve: "continuous",
  },
}));

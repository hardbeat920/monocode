import { StyleSheet } from "react-native";

import { Icon, type IconName } from "./Icon";
import { LiquidView } from "./LiquidView";
import { useColors } from "./theme";

const SIZE = 44;

// Round Liquid Glass icon button. `on`, for a toggle, turns its icon the accent.
export function CircleButton({
  icon,
  label,
  onPress,
  on,
}: {
  icon: IconName;
  label: string;
  onPress?: () => void;
  on?: boolean;
}) {
  const colors = useColors();
  return (
    <LiquidView
      onPress={onPress ?? (() => {})}
      accessibilityLabel={label}
      borderRadius={SIZE / 2}
      innerStyle={styles.surface}
    >
      <Icon name={icon} size={20} color={on ? colors.accent : undefined} />
    </LiquidView>
  );
}

const styles = StyleSheet.create({
  surface: {
    width: SIZE,
    height: SIZE,
  },
});

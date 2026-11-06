import { Button } from "@expo/ui/swift-ui";
import { Platform, StyleSheet } from "react-native";

import { ChoiceMenu } from "@/ui/ChoiceMenu";
import { GlassMenuButton } from "@/ui/GlassMenuButton";
import { Icon } from "@/ui/Icon";
import { LiquidView } from "@/ui/LiquidView";

const SIZE = 44;

/**
 * The round ⋯ button at the right of a Mono's header: a native menu to see
 * its details, start its conversation afresh, or delete it. The screen asks
 * before anything that can't be undone.
 */
export function MonoMenu({
  onDetails,
  onReset,
  onDelete,
}: {
  onDetails: () => void;
  onReset: () => void;
  onDelete: () => void;
}) {
  // The SwiftUI menu has no Android view, so Android lists the actions in a sheet.
  if (Platform.OS === "android") return (
    <ChoiceMenu label="More" actions={[
      { label: "View details", icon: "info", onPress: onDetails },
      { label: "Reset conversation", icon: "refresh", onPress: onReset },
      { label: "Delete Mono", icon: "trash", destructive: true, onPress: onDelete },
    ]}>
      <LiquidView borderRadius={SIZE / 2} innerStyle={styles.round}>
        <Icon name="more" size={20} />
      </LiquidView>
    </ChoiceMenu>
  );
  return (
    <GlassMenuButton icon="more" symbol="ellipsis" label="More" size={SIZE}>
      <Button label="View details" systemImage="info.circle" onPress={onDetails} />
      <Button label="Reset conversation" systemImage="arrow.counterclockwise" onPress={onReset} />
      <Button label="Delete Mono" systemImage="trash" role="destructive" onPress={onDelete} />
    </GlassMenuButton>
  );
}

const styles = StyleSheet.create({
  round: {
    width: SIZE,
    height: SIZE,
  },
});

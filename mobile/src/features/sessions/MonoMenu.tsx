import { Button, Host, HStack, Menu, Spacer } from "@expo/ui/swift-ui";
import { accessibilityLabel, contentShape, frame, shapes } from "@expo/ui/swift-ui/modifiers";
import { StyleSheet, View } from "react-native";

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
  return (
    <View>
      <LiquidView borderRadius={SIZE / 2} innerStyle={styles.round}>
        <Icon name="more" size={20} />
      </LiquidView>
      {/* The native menu lies over the glass, its label an invisible box of
          the button's size, as in SessionFilterMenu. */}
      <Host style={StyleSheet.absoluteFill}>
        <Menu
          modifiers={[accessibilityLabel("More")]}
          label={
            <HStack modifiers={[frame({ width: SIZE, height: SIZE }), contentShape(shapes.circle())]}>
              <Spacer />
            </HStack>
          }
        >
          <Button label="View details" systemImage="info.circle" onPress={onDetails} />
          <Button label="Reset conversation" systemImage="arrow.counterclockwise" onPress={onReset} />
          <Button label="Delete Mono" systemImage="trash" role="destructive" onPress={onDelete} />
        </Menu>
      </Host>
    </View>
  );
}

const styles = StyleSheet.create({
  round: {
    width: SIZE,
    height: SIZE,
  },
});

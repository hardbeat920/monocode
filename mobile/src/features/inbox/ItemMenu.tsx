import { Button } from "@expo/ui/swift-ui";
import type { ComponentProps } from "react";
import { Platform } from "react-native";

import { ChoiceMenu } from "@/ui/ChoiceMenu";
import { GlassMenuButton } from "@/ui/GlassMenuButton";
import { Icon, type IconName } from "@/ui/Icon";
import { LiquidView } from "@/ui/LiquidView";

export type ItemMenuEntry = {
  label: string;
  icon: IconName;
  symbol: ComponentProps<typeof Button>["systemImage"];
  destructive?: boolean;
  onPress: () => void;
};

/** The round ⋯ button in an item's action bar. */
export function ItemMenu({ entries, size }: { entries: ItemMenuEntry[]; size: number }) {
  // The SwiftUI menu has no Android view, so Android lists the actions in a sheet.
  if (Platform.OS === "android") {
    return (
      <ChoiceMenu label="More actions" actions={entries}>
        <LiquidView
          borderRadius={size / 2}
          innerStyle={{ width: size, height: size, alignItems: "center", justifyContent: "center" }}
        >
          <Icon name="more" size={20} />
        </LiquidView>
      </ChoiceMenu>
    );
  }
  return (
    <GlassMenuButton icon="more" symbol="ellipsis" label="More actions" size={size}>
      {entries.map((entry) => (
        <Button
          key={entry.label}
          label={entry.label}
          systemImage={entry.symbol}
          role={entry.destructive ? "destructive" : undefined}
          onPress={entry.onPress}
        />
      ))}
    </GlassMenuButton>
  );
}

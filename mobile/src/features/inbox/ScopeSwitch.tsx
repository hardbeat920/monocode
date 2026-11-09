import { Host, Picker, Text } from "@expo/ui/swift-ui";
import { controlSize, pickerStyle, tag } from "@expo/ui/swift-ui/modifiers";
import * as Haptics from "expo-haptics";
import { View } from "react-native";

import { themedStyles } from "@/ui/theme";

import { INBOX_SCOPES } from "./model";
import type { InboxScope } from "./protocol";

/**
 * All / Needs me / Done, as iOS's own segmented control. A segment holds only
 * text, so the count of what needs you is part of its label.
 */
export function ScopeSwitch({
  value,
  onChange,
  needsMe,
}: {
  value: InboxScope;
  onChange: (value: InboxScope) => void;
  needsMe: number;
}) {
  const styles = useStyles();
  return (
    <View>
      {/* The track is translucent, and the chips and rows scroll under it. */}
      <View pointerEvents="none" style={styles.solid} />
      <Host matchContents={{ vertical: true }}>
        <Picker
          selection={value}
          onSelectionChange={(next) => {
            void Haptics.selectionAsync();
            onChange(next as InboxScope);
          }}
          modifiers={[pickerStyle("segmented"), controlSize("large")]}
        >
          {INBOX_SCOPES.map((scope) => (
            <Text key={scope.id} modifiers={[tag(scope.id)]}>
              {scope.id === "needsMe" && needsMe > 0 ? `${scope.label} ${needsMe}` : scope.label}
            </Text>
          ))}
        </Picker>
      </Host>
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  solid: {
    position: "absolute",
    top: 0,
    bottom: 0,
    left: 0,
    right: 0,
    borderRadius: 999,
    backgroundColor: colors.background,
  },
}));

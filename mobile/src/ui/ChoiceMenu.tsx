import { useState, type ReactNode } from "react";
import {
  Modal,
  Pressable,
  ScrollView,
  Text,
  View,
  type StyleProp,
  type ViewStyle,
} from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { useColors } from "./theme";

export type MenuAction = {
  label: string;
  onPress: () => void;
  selected?: boolean;
  disabled?: boolean;
};
export function ChoiceMenu({
  label,
  children,
  actions,
  disabled,
  style,
}: {
  label: string;
  children: ReactNode;
  actions: MenuAction[];
  disabled?: boolean;
  style?: StyleProp<ViewStyle>;
}) {
  const [open, setOpen] = useState(false);
  const colors = useColors();
  const insets = useSafeAreaInsets();
  return (
    <>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={label}
        disabled={disabled}
        style={style}
        onPress={() => setOpen(true)}
      >
        {children}
      </Pressable>
      <Modal
        visible={open}
        transparent
        animationType="fade"
        onRequestClose={() => setOpen(false)}
      >
        <View
          style={{
            flex: 1,
            justifyContent: "flex-end",
            backgroundColor: "rgba(0,0,0,0.4)",
          }}
        >
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Close menu"
            onPress={() => setOpen(false)}
            style={{ flex: 1 }}
          />
          <ScrollView
            style={{
              // ScrollView defaults to flexGrow: 1, which stretches short menus.
              // Let the title/actions determine height, capped for long lists.
              flexGrow: 0,
              maxHeight: "75%",
              backgroundColor: colors.sheet,
              borderTopLeftRadius: 24,
              borderTopRightRadius: 24,
            }}
            contentContainerStyle={{
              padding: 20,
              paddingBottom: insets.bottom + 20,
            }}
          >
            <Text
              accessibilityRole="header"
              style={{
                color: colors.content,
                fontSize: 20,
                fontWeight: "600",
                marginBottom: 12,
              }}
            >
              {label}
            </Text>
            {actions.map((action, index) => (
              <Pressable
                key={`${index}:${action.label}`}
                accessibilityRole="button"
                accessibilityState={{
                  selected: action.selected,
                  disabled: action.disabled,
                }}
                disabled={action.disabled}
                onPress={() => {
                  setOpen(false);
                  action.onPress();
                }}
                style={{
                  paddingVertical: 16,
                  opacity: action.disabled ? 0.4 : 1,
                }}
              >
                <Text
                  style={{
                    color: action.selected ? colors.accent : colors.content,
                    fontSize: 16,
                  }}
                >
                  {action.selected ? "✓  " : ""}
                  {action.label}
                </Text>
              </Pressable>
            ))}
          </ScrollView>
        </View>
      </Modal>
    </>
  );
}

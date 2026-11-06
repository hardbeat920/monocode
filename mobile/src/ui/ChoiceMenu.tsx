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

import { Icon, type IconName } from "./Icon";
import { themedStyles, useColors } from "./theme";

export type MenuAction = {
  label: string;
  onPress: () => void;
  icon?: IconName;
  selected?: boolean;
  disabled?: boolean;
  /** Drawn in red, as iOS draws a destructive menu action. */
  destructive?: boolean;
};

/**
 * Android's stand-in for a native iOS menu: a bottom sheet in the Workspace
 * sheet's design, its actions in rounded cards. `sections` groups them, a
 * card each, where the iOS menu has dividers.
 */
export function ChoiceMenu({
  label,
  title = label,
  children,
  actions,
  sections = actions ? [actions] : [],
  disabled,
  style,
}: {
  /** The button's accessibility label, and the sheet's title unless given one. */
  label: string;
  title?: string;
  children: ReactNode;
  actions?: MenuAction[];
  sections?: MenuAction[][];
  disabled?: boolean;
  style?: StyleProp<ViewStyle>;
}) {
  const [open, setOpen] = useState(false);
  const styles = useStyles();
  const insets = useSafeAreaInsets();
  const groups = sections.filter((group) => group.length > 0);
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
        statusBarTranslucent
        onRequestClose={() => setOpen(false)}
      >
        <View style={styles.overlay}>
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Close menu"
            onPress={() => setOpen(false)}
            style={styles.backdrop}
          />
          <View accessibilityViewIsModal style={styles.sheet}>
            <View style={styles.handle} />
            <View style={styles.header}>
              <Text accessibilityRole="header" numberOfLines={1} style={styles.title}>
                {title}
              </Text>
            </View>
            <ScrollView
              style={styles.scroll}
              contentContainerStyle={{ ...styles.content, paddingBottom: insets.bottom + 16 }}
              showsVerticalScrollIndicator={false}
            >
              {groups.map((group, index) => (
                <View key={index} style={styles.card}>
                  {group.map((action, row) => (
                    <View key={`${row}:${action.label}`}>
                      {row > 0 ? (
                        <View style={[styles.divider, !action.icon && styles.dividerFlush]} />
                      ) : null}
                      <MenuRow
                        action={action}
                        onPress={() => {
                          setOpen(false);
                          action.onPress();
                        }}
                      />
                    </View>
                  ))}
                </View>
              ))}
            </ScrollView>
          </View>
        </View>
      </Modal>
    </>
  );
}

function MenuRow({ action, onPress }: { action: MenuAction; onPress: () => void }) {
  const styles = useStyles();
  const colors = useColors();
  const tint = action.destructive ? colors.deletions : colors.content;
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={action.label}
      accessibilityState={{ selected: action.selected, disabled: action.disabled }}
      disabled={action.disabled}
      onPress={onPress}
      android_ripple={{ color: colors.selection }}
      style={[styles.row, action.disabled && styles.disabled]}
    >
      {action.icon ? <Icon name={action.icon} size={22} color={tint} /> : null}
      <Text numberOfLines={1} style={[styles.label, { color: tint }]}>
        {action.label}
      </Text>
      {action.selected ? <Icon name="check" size={20} color={colors.accent} /> : null}
    </Pressable>
  );
}

// As WorkspaceMenu.android's sheet.
const useStyles = themedStyles((colors) => ({
  overlay: {
    flex: 1,
    justifyContent: "flex-end",
    backgroundColor: "rgba(0,0,0,0.4)",
  },
  backdrop: { flex: 1 },
  sheet: {
    maxHeight: "80%",
    backgroundColor: colors.sheet,
    borderTopLeftRadius: 32,
    borderTopRightRadius: 32,
    overflow: "hidden",
  },
  handle: {
    width: 40,
    height: 4,
    borderRadius: 2,
    backgroundColor: colors.faint,
    alignSelf: "center",
    marginTop: 8,
  },
  header: {
    height: 56,
    alignItems: "center",
    justifyContent: "center",
    paddingHorizontal: 24,
  },
  title: { color: colors.content, fontSize: 20, fontWeight: "600" },
  scroll: { flexGrow: 0 },
  content: { paddingHorizontal: 16, paddingTop: 4, gap: 12 },
  card: {
    borderRadius: 26,
    backgroundColor: colors.surfaceRaised,
    overflow: "hidden",
  },
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: 14,
    minHeight: 56,
    paddingHorizontal: 16,
    paddingVertical: 12,
  },
  label: { flex: 1, minWidth: 0, fontSize: 17 },
  divider: {
    height: 0.5,
    backgroundColor: colors.stroke,
    marginLeft: 52,
    marginRight: 16,
  },
  dividerFlush: { marginLeft: 16 },
  disabled: { opacity: 0.4 },
}));

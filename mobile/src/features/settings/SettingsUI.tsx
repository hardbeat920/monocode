import type { ReactElement, ReactNode } from "react";
import { useHeaderHeight } from "expo-router/react-navigation";
import {
  Platform,
  Pressable,
  RefreshControl,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { Icon, type IconName } from "@/ui/Icon";
import { radii, themedStyles, useColors } from "@/ui/theme";

/**
 * A settings page's scroll shell. The sheet's transparent native header sits
 * over it, so the content pads below through iOS's automatic insets; under a
 * header of its own, `top` sets the room for it instead.
 */
export function SettingsPage({
  top,
  children,
  onRefresh,
  refreshing = false,
}: {
  top?: number;
  children: ReactNode;
  onRefresh?: () => void;
  refreshing?: boolean;
}) {
  const styles = useStyles();
  const colors = useColors();
  const { bottom } = useSafeAreaInsets();
  // The native height includes the top safe area. Android doesn't apply
  // ScrollView's iOS-only automatic content insets under transparent headers.
  const headerHeight = useHeaderHeight();
  const contentTop = top ?? (Platform.OS === "android" ? headerHeight + 12 : undefined);
  return (
    <ScrollView
      refreshControl={
        onRefresh ? (
          <RefreshControl
            refreshing={refreshing}
            onRefresh={onRefresh}
            tintColor={colors.muted}
            colors={[colors.muted]}
          />
        ) : undefined
      }
      contentInsetAdjustmentBehavior={contentTop === undefined ? "automatic" : "never"}
      showsVerticalScrollIndicator={false}
      contentContainerStyle={[
        styles.page,
        contentTop === undefined ? null : { paddingTop: contentTop },
        { paddingBottom: bottom + 40 },
      ]}
    >
      {children}
    </ScrollView>
  );
}

/**
 * A row: an icon (or any small element) if given, label, an optional value, and a
 * chevron unless told otherwise or given an accessory in its place.
 */
export function SettingsRow({
  icon,
  label,
  value,
  onPress,
  showChevron = true,
  accessory,
  destructive,
}: {
  icon?: IconName | ReactElement;
  label: string;
  value?: string;
  onPress?: () => void;
  showChevron?: boolean;
  accessory?: ReactNode;
  destructive?: boolean;
}) {
  const colors = useColors();
  const styles = useStyles();
  const tint = destructive ? colors.deletions : colors.content;
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      onPress={onPress}
      style={({ pressed }) => [styles.row, pressed && styles.pressed]}
    >
      {icon ? (
        <View style={styles.icon}>
          {typeof icon === "string" ? (
            <Icon name={icon} size={18} color={tint} />
          ) : (
            icon
          )}
        </View>
      ) : null}
      <Text style={[styles.label, { color: tint }]} numberOfLines={1}>
        {label}
      </Text>
      {value ? <Text style={styles.value}>{value}</Text> : null}
      {accessory ??
        (showChevron ? (
          <Icon name="chevronRight" size={18} color={colors.faint} />
        ) : null)}
    </Pressable>
  );
}

/** A hairline between groups of rows. */
export function SettingsDivider() {
  const styles = useStyles();
  return <View style={styles.divider} />;
}

const useStyles = themedStyles((colors) => ({
  page: {
    paddingHorizontal: 20,
    paddingTop: 12,
  },
  row: {
    flexDirection: "row",
    alignItems: "center",
    // As tall without an icon as with one.
    minHeight: 52,
    paddingVertical: 8,
    borderRadius: radii.row,
  },
  pressed: {
    opacity: 0.6,
  },
  icon: {
    width: 36,
    height: 36,
    marginRight: 4,
    alignItems: "center",
    justifyContent: "center",
  },
  label: {
    flex: 1,
    fontSize: 17,
  },
  value: {
    color: colors.muted,
    fontSize: 14,
    marginRight: 8,
  },
  divider: {
    height: StyleSheet.hairlineWidth,
    backgroundColor: colors.stroke,
    marginVertical: 8,
  },
}));

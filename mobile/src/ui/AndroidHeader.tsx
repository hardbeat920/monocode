import { router, type NativeStackNavigationOptions } from "expo-router";
import type { ReactNode } from "react";
import { Platform, Pressable, Text, View } from "react-native";
import Svg, { Path } from "react-native-svg";

import { themedStyles, useColors } from "./theme";

/** Keep the back target and title together, without Android toolbar's extra title inset. */
export function androidBackHeader(title: ReactNode): NativeStackNavigationOptions {
  if (Platform.OS !== "android") return {};
  return {
    headerBackVisible: false,
    headerLeft: undefined,
    headerTitle: () => <AndroidBackTitle>{title}</AndroidBackTitle>,
  };
}

function AndroidBackTitle({ children }: { children: ReactNode }) {
  const colors = useColors();
  const styles = useStyles();
  return (
    <View style={styles.row}>
      <Pressable accessibilityRole="button" accessibilityLabel="Back"
        onPress={() => router.back()} style={styles.back}>
        <Svg width={24} height={24} viewBox="0 0 24 24">
          <Path d="M20 12H4m7-7-7 7 7 7" fill="none" stroke={colors.content}
            strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" />
        </Svg>
      </Pressable>
      <View style={styles.title}>
        {typeof children === "string" ? (
          <Text numberOfLines={1} style={styles.label}>{children}</Text>
        ) : children}
      </View>
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  row: { flex: 1, minWidth: 0, flexDirection: "row", alignItems: "center", gap: 4 },
  back: { width: 40, height: 48, alignItems: "center", justifyContent: "center" },
  title: { flex: 1, minWidth: 0 },
  label: { color: colors.content, fontSize: 20, fontWeight: "600" },
}));

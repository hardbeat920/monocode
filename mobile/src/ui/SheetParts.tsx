import type { ReactNode } from "react";
import { Text, View, type StyleProp, type ViewStyle } from "react-native";

import { themedStyles } from "./theme";

/** What every sheet's content keeps from the sheet's edge. */
export const SHEET_INSET = 16;
/** Above a sheet's title, clear of the drag indicator. */
export const SHEET_TOP = 24;
export const SHEET_ROW = 52;

/** A sheet's name, centred, with a line under it when it needs one. */
export function SheetTitle({ title, detail }: { title: string; detail?: string }) {
  const styles = useSheetStyles();
  return (
    <View style={styles.titleBlock}>
      <Text accessibilityRole="header" numberOfLines={1} style={styles.title}>
        {title}
      </Text>
      {detail ? <Text style={styles.detail}>{detail}</Text> : null}
    </View>
  );
}

/** The name over a card. */
export function SheetHeading({ children }: { children: string }) {
  const styles = useSheetStyles();
  return <Text style={styles.heading}>{children}</Text>;
}

/** Rows grouped on the glass, as iOS's inset grouped lists are. */
export function SheetCard({
  children,
  style,
}: {
  children: ReactNode;
  style?: StyleProp<ViewStyle>;
}) {
  const styles = useSheetStyles();
  return <View style={[styles.card, style]}>{children}</View>;
}

/** The line between two rows, starting under their text. */
export function SheetDivider({ inset = SHEET_INSET }: { inset?: number }) {
  const styles = useSheetStyles();
  return <View style={[styles.divider, { marginLeft: inset }]} />;
}

export const useSheetStyles = themedStyles((colors) => ({
  titleBlock: {
    gap: 4,
  },
  title: {
    color: colors.content,
    fontSize: 17,
    fontWeight: "600",
    textAlign: "center",
  },
  detail: {
    color: colors.muted,
    fontSize: 14,
    lineHeight: 19,
    textAlign: "center",
  },
  heading: {
    paddingTop: 16,
    paddingBottom: 6,
    paddingHorizontal: SHEET_INSET,
    color: colors.muted,
    fontSize: 13,
    fontWeight: "500",
  },
  card: {
    borderRadius: 20,
    borderCurve: "continuous",
    backgroundColor: colors.fill,
    overflow: "hidden",
  },
  divider: {
    height: 1,
    backgroundColor: colors.stroke,
  },
  /** A text field on the glass. */
  field: {
    minHeight: 48,
    paddingHorizontal: SHEET_INSET,
    borderRadius: 16,
    borderCurve: "continuous",
    backgroundColor: colors.fill,
    color: colors.content,
    fontSize: 16,
  },
  /** The sheet's main button. */
  primary: {
    height: 50,
    borderRadius: 25,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.inverseSurface,
  },
  primaryLabel: {
    color: colors.inverse,
    fontSize: 16,
    fontWeight: "600",
  },
  dimmed: {
    opacity: 0.4,
  },
  pressed: {
    opacity: 0.7,
  },
}));

import { Text, View } from "react-native";

import { themedStyles } from "@/ui/theme";

/** The unread count at the end of the Inbox's row; nothing at none. */
export function InboxCount({ unread }: { unread: number }) {
  const styles = useStyles();
  if (unread <= 0) return null;
  return (
    <View style={styles.pill}>
      <Text style={styles.label}>{unread > 99 ? "99+" : unread}</Text>
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  pill: {
    minWidth: 22,
    height: 22,
    paddingHorizontal: 6,
    borderRadius: 11,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.accent,
  },
  label: {
    color: colors.inverse,
    fontSize: 13,
    fontWeight: "700",
    fontVariant: ["tabular-nums"],
  },
}));

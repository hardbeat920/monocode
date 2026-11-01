import { Text, View } from "react-native";

import { Icon } from "@/ui/Icon";
import { themedStyles } from "@/ui/theme";

/**
 * The Inbox segment's icon with its unread count at the top right. The count
 * floats over the icon without taking room, so the switch measures the same
 * with or without it.
 */
export function InboxSegmentIcon({ unread }: { unread: number }) {
  const styles = useStyles();
  return (
    <View>
      <Icon name="inbox" size={18} />
      {unread > 0 ? (
        <View
          accessibilityLabel={`${unread} unread`}
          pointerEvents="none"
          style={styles.badge}
        >
          <Text style={styles.label}>{unread > 99 ? "99+" : unread}</Text>
        </View>
      ) : null}
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  badge: {
    position: "absolute",
    top: -7,
    right: -9,
    minWidth: 16,
    height: 16,
    paddingHorizontal: 4,
    borderRadius: 8,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.accent,
  },
  label: {
    color: colors.inverse,
    fontSize: 10,
    fontWeight: "700",
    fontVariant: ["tabular-nums"],
  },
}));

import { View } from "react-native";

import { useConnection } from "@/features/connection/ConnectionProvider";
import { themedStyles } from "@/ui/theme";

import { useUnreadMonos } from "./monoSeen";
import { useWorkspace } from "./WorkspaceProvider";

/** Whether any mono has a reply the user hasn't seen, for the drawer's button. */
export function useAnyUnreadMono() {
  const { monos } = useWorkspace();
  const desktop = useConnection().pairing?.desktop.fingerprint;
  return useUnreadMonos(desktop, monos).size > 0;
}

/** The unread dot on the drawer's menu button, at its top right. */
export function UnreadDot() {
  const styles = useStyles();
  return <View pointerEvents="none" style={styles.dot} />;
}

const useStyles = themedStyles((colors) => ({
  dot: {
    position: "absolute",
    top: 2,
    right: 2,
    width: 10,
    height: 10,
    borderRadius: 5,
    borderWidth: 2,
    borderColor: colors.background,
    backgroundColor: colors.accent,
  },
}));

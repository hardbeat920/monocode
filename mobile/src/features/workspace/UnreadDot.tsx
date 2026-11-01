import { View } from "react-native";

import { useConnection } from "@/features/connection/ConnectionProvider";
import { useFinishedCount } from "@/features/sessions/activityStore";
import { themedStyles } from "@/ui/theme";

import { useUnreadMonos } from "./monoSeen";
import { useWorkspace } from "./WorkspaceProvider";

/** How many things wait to be seen: Monos with a reply, and finished sessions
 *  not yet opened, as the Sessions page shelves them. */
export function unreadTotal(unreadMonos: number, finishedSessions: number) {
  return unreadMonos + finishedSessions;
}

/** Whether anything is unread, for the drawer button's dot. Monos no longer
 *  have a place in the drawer, so their replies dot the button that opens it. */
export function useAnyUnread() {
  const { monos } = useWorkspace();
  const desktop = useConnection().pairing?.desktop.fingerprint;
  const unreadMonos = useUnreadMonos(desktop, monos).size;
  return unreadTotal(unreadMonos, useFinishedCount()) > 0;
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

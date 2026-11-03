import { useEffect } from "react";

import { useConnection } from "@/features/connection/ConnectionProvider";

import { resetSessionActivity, trackSessions } from "./activityStore";

/**
 * Follows the desktop's session list as the phone polls it, wherever the
 * app is, so a session that finishes while another project is open is still
 * on its deck when that project comes back. Mounted once, per pairing.
 */
export function SessionActivity() {
  const { connected, catalog } = useConnection();
  useEffect(() => {
    if (connected) trackSessions(catalog.sessions);
  }, [connected, catalog]);
  useEffect(() => resetSessionActivity, []);
  return null;
}

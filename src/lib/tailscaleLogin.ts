/**
 * Device registration URL from tsnet/tailscale-rs (`/a/…`).
 * Must not go through /logout?next= — that drops the node auth and lands
 * on the admin console, so the iPad never joins.
 */
export function googleSignInHref(loginUrl: string): string {
  try {
    const parsed = new URL(loginUrl);
    if (parsed.hostname !== "login.tailscale.com") return loginUrl;
    if (parsed.pathname === "/logout") {
      const next = parsed.searchParams.get("next");
      if (next) return googleSignInHref(next);
    }
    return loginUrl;
  } catch {
    return loginUrl;
  }
}

/** True when this is the per-device auth page, not the admin console. */
export function isTailscaleDeviceAuthUrl(loginUrl: string): boolean {
  try {
    const parsed = new URL(googleSignInHref(loginUrl));
    return (
      parsed.hostname === "login.tailscale.com" &&
      parsed.pathname.startsWith("/a/")
    );
  } catch {
    return false;
  }
}

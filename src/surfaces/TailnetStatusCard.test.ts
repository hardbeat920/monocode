import { describe, expect, it } from "vitest";
import { googleSignInHref } from "../lib/tailscaleLogin";

describe("googleSignInHref", () => {
  it("keeps the device auth URL so Connect this device is shown", () => {
    const auth = "https://login.tailscale.com/a/abc123";
    expect(googleSignInHref(auth)).toBe(auth);
  });

  it("unwraps a leftover logout?next= wrapper", () => {
    const auth = "https://login.tailscale.com/a/abc123";
    const wrapped = `https://login.tailscale.com/logout?next=${encodeURIComponent(auth)}`;
    expect(googleSignInHref(wrapped)).toBe(auth);
  });

  it("leaves non-Tailscale URLs alone", () => {
    expect(googleSignInHref("https://example.com/login")).toBe(
      "https://example.com/login",
    );
  });
});

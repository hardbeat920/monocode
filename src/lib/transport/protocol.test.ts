import { describe, expect, it } from "vitest";
import {
  buildCompanionWsUrl,
  buildPairUrl,
  companionDialUrls,
  COMPANION_PORT_DEFAULT,
  decodeBytesEnvelope,
  isBytesEnvelope,
  isLocalOnlyCommand,
  isPairingToken,
  isTailnetHost,
  otherPairingHost,
  pairingDialOrder,
  parsePairUrl,
} from "./protocol";

describe("pairing urls", () => {
  it("round-trips host, port, and token", () => {
    const details = {
      host: "macbook.tail9a5.ts.net",
      port: COMPANION_PORT_DEFAULT,
      token: "abcDEF0123456789-_abcDEF0123456789",
    };
    const parsed = parsePairUrl(buildPairUrl(details));
    expect(parsed).toEqual(details);
  });

  it("round-trips an alternate host for dual LAN+Tailscale pairing", () => {
    const details = {
      host: "192.168.1.20",
      port: COMPANION_PORT_DEFAULT,
      token: "0123456789abcdef0123456789abcdef",
      altHost: "macbook.tail9a5.ts.net",
    };
    const parsed = parsePairUrl(buildPairUrl(details));
    expect(parsed).toEqual(details);
    expect(buildPairUrl(details)).toContain("alt=macbook.tail9a5.ts.net");
  });

  it("round-trips tailscale (secure) details", () => {
    const details = {
      host: "macbook.tail9a5.ts.net",
      port: 443,
      token: "0123456789abcdef0123456789abcdef",
      secure: true as const,
    };
    expect(parsePairUrl(buildPairUrl(details))).toEqual(details);
    expect(buildCompanionWsUrl(details)).toBe(
      "wss://macbook.tail9a5.ts.net:443/v1/connect?token=0123456789abcdef0123456789abcdef&v=1",
    );
  });

  it("treats legacy urls without a secure flag as plain ws", () => {
    const parsed = parsePairUrl(
      "monocode://pair?host=192.168.1.20&port=17233&token=0123456789abcdef0123456789abcdef&v=1",
    );
    expect(parsed).toEqual({
      host: "192.168.1.20",
      port: 17233,
      token: "0123456789abcdef0123456789abcdef",
    });
    expect(parsed?.secure).toBeUndefined();
  });

  it("rejects foreign schemes and malformed payloads", () => {
    expect(parsePairUrl("https://example.com/?token=x")).toBeNull();
    expect(parsePairUrl("monocode://pair?host=&port=17233&token=abc")).toBeNull();
    expect(parsePairUrl("monocode://pair?host=h&port=99999&token=abc")).toBeNull();
    expect(
      parsePairUrl("monocode://pair?host=h&port=17233&token=short"),
    ).toBeNull();
    expect(parsePairUrl("monocode://pair?host=h&port=17233")).toBeNull();
  });

  it("builds a ws url carrying the token", () => {
    const url = buildCompanionWsUrl({
      host: "192.168.1.20",
      port: 17233,
      token: "0123456789abcdef0123456789abcdef",
    });
    expect(url).toBe(
      "ws://192.168.1.20:17233/v1/connect?token=0123456789abcdef0123456789abcdef&v=1",
    );
  });
});

describe("isTailnetHost", () => {
  it("recognizes CGNAT, MagicDNS, and Tailscale IPv6", () => {
    expect(isTailnetHost("100.80.151.26")).toBe(true);
    expect(isTailnetHost("mac.tail9a5.ts.net")).toBe(true);
    expect(isTailnetHost("[fd7a:115c:a1e0::1]")).toBe(true);
    expect(isTailnetHost("192.168.4.191")).toBe(false);
    expect(isTailnetHost("10.0.0.1")).toBe(false);
  });
});

describe("pairingDialOrder", () => {
  it("tries LAN before the tailnet address", () => {
    const details = {
      host: "100.80.151.26",
      port: 17233,
      token: "0123456789abcdef0123456789abcdef",
      altHost: "192.168.4.191",
    };
    expect(pairingDialOrder(details).map((item) => item.host)).toEqual([
      "192.168.4.191",
      "100.80.151.26",
    ]);
    expect(companionDialUrls(details)).toEqual([
      "ws://192.168.4.191:17233/v1/connect?token=0123456789abcdef0123456789abcdef&v=1",
      "ws://100.80.151.26:17233/v1/connect?token=0123456789abcdef0123456789abcdef&v=1",
    ]);
  });
});

describe("otherPairingHost", () => {
  it("returns the advertised host that is not the one already connected", () => {
    expect(
      otherPairingHost("192.168.1.20", {
        lanIp: "192.168.1.20",
        tailnetHost: "mac.tail.ts.net",
      }),
    ).toBe("mac.tail.ts.net");
    expect(
      otherPairingHost("mac.tail.ts.net", {
        lanIp: "192.168.1.20",
        tailnetHost: "mac.tail.ts.net",
      }),
    ).toBe("192.168.1.20");
    expect(
      otherPairingHost("192.168.1.20", { lanIp: "192.168.1.20" }),
    ).toBeUndefined();
  });
});

describe("pairing tokens", () => {
  it("accepts url-safe tokens, rejects everything else", () => {
    expect(isPairingToken("0123456789abcdef")).toBe(true);
    expect(isPairingToken("a".repeat(15))).toBe(false);
    expect(isPairingToken("has space in it 12345678")).toBe(false);
    expect(isPairingToken("semi;colon?query=12345678")).toBe(false);
  });
});

describe("binary envelopes", () => {
  it("round-trips bytes through base64", () => {
    // "hi" as base64.
    const envelope = { __bytes: "aGk=" };
    expect(isBytesEnvelope(envelope)).toBe(true);
    expect(isBytesEnvelope({ __bytes: 42 })).toBe(false);
    expect(isBytesEnvelope({ __bytes: "aGk=", extra: 1 })).toBe(false);
    expect(isBytesEnvelope(null)).toBe(false);
    const buffer = decodeBytesEnvelope(envelope);
    expect(Array.from(new Uint8Array(buffer))).toEqual([104, 105]);
  });
});

describe("command routing", () => {
  it("keeps window chrome local but forwards agent and pty traffic", () => {
    for (const command of [
      "set_dock_badge",
      "hide_window",
      "destroy_window",
      "confirm_quit",
      "open_new_window",
      "enable_window_glass",
      "set_traffic_lights_visible",
      "set_window_background_blur",
    ]) {
      expect(isLocalOnlyCommand(command)).toBe(true);
    }
    for (const command of [
      "harness_spawn",
      "harness_write",
      "harness_kill",
      "harness_http",
      "harness_sse_open",
      "pty_spawn",
      "pty_write",
      "session_upsert",
      "session_list_by_project",
      "workspace_set_snapshot",
      "list_dir",
      "git_diff_stats",
      "search_project",
      "notes_list",
      // Unknown future upstream commands default to forwarded, never dropped.
      "some_future_command",
    ]) {
      expect(isLocalOnlyCommand(command)).toBe(false);
    }
  });
});

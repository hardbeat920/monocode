import { describe, expect, it } from "vitest";
import {
  openClawGatewayConfig,
  openClawSessionKey,
  openClawTransport,
} from "./openclawTransport";

describe("OpenClaw transport boundary", () => {
  it("uses the fixed ACP bridge argv", () => {
    expect(openClawTransport("/usr/local/bin/openclaw")).toMatchObject({
      provider: "openclaw",
      path: "/usr/local/bin/openclaw",
      args: ["acp"],
    });
  });

  it("does not model secrets as frontend configuration", () => {
    expect(openClawGatewayConfig(" wss://gateway.example ")).toEqual({
      url: "wss://gateway.example",
      secretSource: "native-runtime",
    });
  });

  it("namespaces gateway session keys", () => {
    expect(openClawSessionKey("team/session")).toBe("acp-bridge:team/session");
  });
});

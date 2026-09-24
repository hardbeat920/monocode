import { describe, expect, it } from "vitest";
import {
  openClawConnectRequest,
  openClawRequest,
  parseOpenClawFrame,
} from "./openclawGatewayProtocol";

describe("OpenClaw Gateway protocol", () => {
  it("parses hello, request, response and event frames", () => {
    expect(parseOpenClawFrame('{"type":"hello-ok","snapshot":{}}')).toMatchObject({ type: "hello-ok" });
    expect(parseOpenClawFrame('{"type":"req","id":"1","method":"chat.send"}')).toMatchObject({ type: "req", id: "1" });
    expect(parseOpenClawFrame('{"type":"res","id":"1","ok":true}')).toMatchObject({ type: "res", id: "1", ok: true });
    expect(parseOpenClawFrame('{"type":"event","event":"heartbeat"}')).toMatchObject({ type: "event", event: "heartbeat" });
  });

  it("rejects malformed or unsupported frames", () => {
    expect(() => parseOpenClawFrame("not-json")).toThrow(/invalid JSON/i);
    expect(() => parseOpenClawFrame('{"type":"req","id":1,"method":"x"}')).toThrow(/unsupported/i);
  });

  it("builds request and connect frames without credentials", () => {
    expect(openClawRequest("r1", "chat.send", { text: "hi" })).toEqual({
      type: "req", id: "r1", method: "chat.send", params: { text: "hi" },
    });
    expect(openClawConnectRequest("connect-1", { id: "monocode" })).toEqual({
      type: "req", id: "connect-1", method: "connect", params: { client: { id: "monocode" } },
    });
  });
});

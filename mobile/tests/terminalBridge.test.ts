import { describe, expect, it } from "vitest";

import {
  parsePageMessage,
  run,
  writeScript,
} from "@/features/terminal/terminalBridge";

describe("messages from the terminal page", () => {
  it("accepts the four the page sends", () => {
    expect(parsePageMessage('{"t":"ready"}')).toEqual({ t: "ready" });
    expect(parsePageMessage('{"t":"input","data":"bHMK"}')).toEqual({
      t: "input",
      data: "bHMK",
    });
    expect(parsePageMessage('{"t":"size","cols":80,"rows":24}')).toEqual({
      t: "size",
      cols: 80,
      rows: 24,
    });
    expect(parsePageMessage('{"t":"full","on":true}')).toEqual({ t: "full", on: true });
  });

  it("drops anything else, since the page shows a remote shell's output", () => {
    for (const raw of [
      undefined,
      42,
      "not json",
      "null",
      '"ready"',
      '{"t":"open","url":"https://example.com"}',
      '{"t":"input","data":"not base64!"}',
      '{"t":"input","data":5}',
      '{"t":"size","cols":0,"rows":24}',
      '{"t":"size","cols":80.5,"rows":24}',
      '{"t":"size","cols":80,"rows":9999}',
      '{"t":"size","cols":"80","rows":24}',
      '{"t":"full","on":"yes"}',
      JSON.stringify({ t: "input", data: "A".repeat(200 * 1024) }),
    ])
      expect(parsePageMessage(raw), String(raw).slice(0, 40)).toBeNull();
  });
});

describe("scripts for the terminal page", () => {
  it("quotes output so it cannot end the script early", () => {
    const script = writeScript('a"b\\c', true);
    expect(script).toBe('window.__mono.write("a\\"b\\\\c",true);');
    expect(run([script, "window.__mono.focus();"])).toMatch(/true;$/);
  });
});

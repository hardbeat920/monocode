import { describe, expect, it, vi } from "vitest";

import { textBytes } from "@/features/terminal/base64";
import { actionBytes, withCtrl } from "@/features/terminal/keys";

vi.mock("expo-sqlite/kv-store", () => ({
  default: { getItem: vi.fn(), setItem: vi.fn() },
}));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({ pairing: null }),
}));

describe("terminal keys", () => {
  it("turns a letter into its control code and leaves the rest alone", () => {
    expect(Array.from(withCtrl(textBytes("c")))).toEqual([3]);
    expect(Array.from(withCtrl(textBytes("C")))).toEqual([3]);
    expect(Array.from(withCtrl(textBytes("[")))).toEqual([27]);
    expect(Array.from(withCtrl(textBytes(" ")))).toEqual([0]);
    expect(Array.from(withCtrl(textBytes("1")))).toEqual([49]);
    expect(Array.from(withCtrl(textBytes("ab")))).toEqual([97, 98]);
  });

  it("types an action and presses return", () => {
    expect(new TextDecoder().decode(actionBytes("git status"))).toBe(
      "git status\r",
    );
  });
});

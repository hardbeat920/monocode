import { describe, expect, it } from "vitest";
import { translate } from "./locale";

describe("translate", () => {
  it("keeps English as the source text", () => {
    expect(translate("Settings", undefined, "en")).toBe("Settings");
  });

  it("returns simplified Chinese for a known string", () => {
    expect(translate("Settings", undefined, "zh-CN")).toBe("设置");
    expect(translate("Language", undefined, "zh-CN")).toBe("语言");
  });

  it("falls back to English when a string has no translation", () => {
    expect(translate("Not in the catalog", undefined, "zh-CN")).toBe(
      "Not in the catalog",
    );
  });

  it("fills named placeholders", () => {
    expect(translate("{count} sessions", { count: 2 }, "zh-CN")).toBe(
      "2 个会话",
    );
  });
});

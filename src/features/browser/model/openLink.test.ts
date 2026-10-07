import { describe, expect, it } from "vitest";
import { linkTarget } from "./openLink";

const plain = { metaKey: false, ctrlKey: false };
const cmd = { metaKey: true, ctrlKey: false };
const ctrl = { metaKey: false, ctrlKey: true };

describe("linkTarget", () => {
  it("follows the setting for a plain click", () => {
    expect(linkTarget("https://example.com", true, plain)).toBe("browser");
    expect(linkTarget("https://example.com", false, plain)).toBe("external");
    expect(linkTarget("https://example.com", true)).toBe("browser");
  });

  it("flips with the platform modifier", () => {
    expect(linkTarget("https://example.com", true, cmd, true)).toBe("external");
    expect(linkTarget("https://example.com", false, cmd, true)).toBe("browser");
    expect(linkTarget("https://example.com", true, ctrl, true)).toBe("browser");
    expect(linkTarget("https://example.com", true, ctrl, false)).toBe(
      "external",
    );
  });

  it("sends anything but web pages to the system", () => {
    expect(linkTarget("mailto:a@b.c", true, plain)).toBe("external");
    expect(linkTarget("file:///etc/hosts", true, cmd, true)).toBe("external");
  });
});

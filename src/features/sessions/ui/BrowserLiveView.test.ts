import { describe, expect, it } from "vitest";
import {
  liveAddress,
  liveKeyInput,
  liveModifiers,
  livePoint,
} from "./BrowserLiveView";

const key = (
  key: string,
  extra: Partial<{
    code: string;
    ctrlKey: boolean;
    metaKey: boolean;
    altKey: boolean;
    shiftKey: boolean;
    isComposing: boolean;
  }> = {},
) => ({
  key,
  code: extra.code ?? "",
  altKey: false,
  ctrlKey: false,
  metaKey: false,
  shiftKey: false,
  isComposing: false,
  ...extra,
});

describe("live view input", () => {
  it("maps a click on the shown picture to the page's CSS pixels", () => {
    const picture = { left: 100, top: 50, width: 500, height: 250 };
    const page = { width: 1000, height: 500 };
    expect(livePoint({ x: 350, y: 175 }, picture, page)).toEqual({
      x: 500,
      y: 250,
    });
    // Outside the picture: held to its edge.
    expect(livePoint({ x: 0, y: 900 }, picture, page)).toEqual({
      x: 0,
      y: 500,
    });
  });

  it("sends editing keys and shortcuts, and leaves typing to the text field", () => {
    expect(liveKeyInput(key("Enter", { code: "Enter" }), "down")).toEqual({
      kind: "key",
      event: "keyDown",
      key: "Enter",
      code: "Enter",
      text: "\r",
      keyCode: 13,
      modifiers: 0,
    });
    expect(liveKeyInput(key("Backspace"), "down")).toMatchObject({
      event: "rawKeyDown",
      keyCode: 8,
    });
    expect(liveKeyInput(key("ArrowLeft"), "up")).toMatchObject({
      event: "keyUp",
      keyCode: 37,
    });
    expect(
      liveKeyInput(key("a", { code: "KeyA", ctrlKey: true }), "down"),
    ).toMatchObject({ event: "rawKeyDown", keyCode: 65, modifiers: 2 });
    // Letters, Serbian ones included, arrive as text; Ctrl+V pastes as text.
    expect(liveKeyInput(key("ć"), "down")).toBeNull();
    expect(liveKeyInput(key("v", { ctrlKey: true }), "down")).toBeNull();
    expect(
      liveKeyInput(key("Enter", { isComposing: true }), "down"),
    ).toBeNull();
  });

  it("encodes modifiers the way CDP does", () => {
    expect(
      liveModifiers({
        altKey: true,
        ctrlKey: true,
        metaKey: true,
        shiftKey: true,
      }),
    ).toBe(15);
  });

  it("shows an address with its host first and the rest quieter", () => {
    expect(liveAddress("https://shop.example.com/cart?x=1")).toEqual({
      secure: true,
      host: "shop.example.com",
      rest: "/cart?x=1",
    });
    expect(liveAddress("http://localhost:5173/")).toEqual({
      secure: false,
      host: "localhost:5173",
      rest: "",
    });
    expect(liveAddress("about:blank").host).toBe("about:blank");
  });
});

import { describe, expect, it } from "vitest";
import {
  type BrowserLiveInput,
  parseBrowserLiveFrame,
} from "../../settings/model/browser";
import {
  CLICK_SLOP,
  isNewTabKey,
  liveAddress,
  liveKeyInput,
  liveModifiers,
  livePoint,
  pixelSnap,
  queueLiveInput,
  wheelPixels,
  withinClickSlop,
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

  it("merges moves and scrolls that wait, keeping everything else in order", () => {
    const queue: BrowserLiveInput[] = [];
    const wheel = (deltaY: number, modifiers = 0): BrowserLiveInput => ({
      kind: "wheel",
      x: 10,
      y: deltaY,
      deltaX: 0,
      deltaY,
      modifiers,
    });
    const move = (x: number): BrowserLiveInput => ({
      kind: "mouse",
      event: "mouseMoved",
      x,
      y: 0,
      buttons: 0,
      modifiers: 0,
    });
    queueLiveInput(queue, wheel(40));
    queueLiveInput(queue, wheel(60));
    queueLiveInput(queue, wheel(-30));
    expect(queue).toEqual([{ ...wheel(-30), deltaY: 70 }]);
    // Ctrl+wheel zooms: never folded into a plain scroll.
    queueLiveInput(queue, wheel(10, 2));
    queueLiveInput(queue, move(1));
    queueLiveInput(queue, move(2));
    queueLiveInput(queue, { kind: "text", text: "a" });
    queueLiveInput(queue, move(3));
    expect(queue.map((input) => input.kind)).toEqual([
      "wheel",
      "wheel",
      "mouse",
      "text",
      "mouse",
    ]);
    expect(queue[2]).toEqual(move(2));
  });

  it("keeps a shaking click a click, and a longer move a drag", () => {
    const pressed = { x: 100, y: 100 };
    // Chromium drags a link from 4 px: a hand moves that much clicking.
    expect(withinClickSlop(pressed, { x: 104, y: 103 })).toBe(true);
    expect(withinClickSlop(pressed, { x: 100 + CLICK_SLOP, y: 100 })).toBe(
      false,
    );
    expect(withinClickSlop(pressed, { x: 80, y: 130 })).toBe(false);
  });

  it("takes Ctrl+T and Cmd+T for a new tab, and nothing else", () => {
    const keys = {
      altKey: false,
      ctrlKey: false,
      metaKey: false,
      shiftKey: false,
    };
    expect(isNewTabKey({ ...keys, key: "t", ctrlKey: true })).toBe(true);
    expect(isNewTabKey({ ...keys, key: "T", metaKey: true })).toBe(true);
    expect(isNewTabKey({ ...keys, key: "t" })).toBe(false);
    // Ctrl+Shift+T reopens a closed tab in a browser; not here.
    expect(
      isNewTabKey({ ...keys, key: "T", ctrlKey: true, shiftKey: true }),
    ).toBe(false);
    expect(isNewTabKey({ ...keys, key: "r", ctrlKey: true })).toBe(false);
  });

  it("counts wheel lines and pages in pixels", () => {
    expect(wheelPixels(120, 0, 900)).toBe(120);
    expect(wheelPixels(3, 1, 900)).toBe(120);
    expect(wheelPixels(1, 2, 900)).toBe(900);
  });

  it("moves the picture onto whole device pixels", () => {
    expect(pixelSnap(100, 1)).toBe(0);
    // 100.5 CSS px at 150% is 150.75 device px: a quarter pixel on.
    expect(pixelSnap(100.5, 1.5) * 1.5 + 100.5 * 1.5).toBeCloseTo(151);
    expect(Math.abs(pixelSnap(33.3, 1.25))).toBeLessThan(0.5);
  });
});

describe("live view pictures", () => {
  it("reads a picture the way the browser module packs it", () => {
    const picture = [0xff, 0xd8, 0xff, 0x00];
    const buffer = new ArrayBuffer(24 + picture.length);
    const view = new DataView(buffer);
    view.setUint8(0, 1);
    view.setUint32(4, 42, true);
    view.setFloat64(8, 800.5, true);
    view.setFloat64(16, 600, true);
    new Uint8Array(buffer, 24).set(picture);
    const frame = parseBrowserLiveFrame(buffer)!;
    expect(frame).toMatchObject({
      format: "png",
      seq: 42,
      width: 800.5,
      height: 600,
    });
    expect([...frame.picture]).toEqual(picture);
    view.setUint8(0, 0);
    expect(parseBrowserLiveFrame(buffer)!.format).toBe("jpeg");
    expect(parseBrowserLiveFrame(new ArrayBuffer(24))).toBeNull();
  });
});

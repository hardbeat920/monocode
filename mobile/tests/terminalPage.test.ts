// @vitest-environment node
import { createRequire } from "node:module";
import { existsSync } from "node:fs";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import { fromBase64, toBase64, textBytes } from "@/features/terminal/base64";
import { parsePageMessage, type PageMessage } from "@/features/terminal/terminalBridge";
import { TERMINAL_PAGE } from "@/features/terminal/terminalPage.generated";

// The page the phone's WebView loads, run in a phone-sized Chromium with the
// app's side of the bridge stood in for. Skipped where Chromium isn't installed
// (`npx playwright install chromium`).
const { chromium, devices } = createRequire(import.meta.url)("../../node_modules/playwright");
const installed = existsSync(chromium.executablePath());

type Page = import("playwright").Page;
let browser: import("playwright").Browser;
let page: Page;
const messages: PageMessage[] = [];

const write = (text: string, reset = false) =>
  page.evaluate(([data, clear]) => window.__mono.write(data as string, clear as boolean), [
    toBase64(textBytes(text)),
    reset,
  ]);
const rows = async () => {
  await page.waitForTimeout(120);
  return page.evaluate(() =>
    document.querySelector(".xterm-rows")!.textContent!.replace(/ /g, " "),
  );
};

declare global {
  interface Window {
    __mono: {
      write: (data: string, reset: boolean) => void;
      paste: (data: string) => void;
      inset: (px: number) => void;
    };
  }
}

describe.skipIf(!installed)("the terminal page", () => {
  beforeAll(async () => {
    browser = await chromium.launch();
    const context = await browser.newContext({ ...devices["iPhone 14"] });
    page = await context.newPage();
    await page.exposeFunction("__post", (raw: string) => {
      const message = parsePageMessage(raw);
      if (message) messages.push(message);
    });
    await page.addInitScript(() => {
      (window as unknown as { ReactNativeWebView: unknown }).ReactNativeWebView = {
        postMessage: (raw: string) => (window as never as { __post: (raw: string) => void }).__post(raw),
      };
    });
    await page.route("https://terminal.test/", (route) =>
      route.fulfill({ contentType: "text/html", body: TERMINAL_PAGE }),
    );
    await page.goto("https://terminal.test/");
    await page.waitForTimeout(300);
  });
  afterAll(() => browser?.close());

  it("says it is ready and how many columns and rows fit", () => {
    expect(messages).toContainEqual({ t: "ready" });
    const size = messages.find((message) => message.t === "size");
    expect(size).toMatchObject({ t: "size" });
    expect((size as { cols: number }).cols).toBeGreaterThan(30);
  });

  it("draws output, UTF-8 included, and clears the screen on a reset", async () => {
    await write("\x1b[32mhello\x1b[0m wörld ☃\r\nline two", true);
    const text = await rows();
    expect(text).toContain("hello wörld ☃");
    expect(text).toContain("line two");
    await write("fresh", true);
    expect(await rows()).not.toContain("hello");
  });

  it("reports typing as base64 bytes", async () => {
    await page.locator("body").click();
    await page.keyboard.type("é");
    await page.keyboard.press("Enter");
    await page.waitForTimeout(100);
    const typed = messages
      .filter((message): message is Extract<PageMessage, { t: "input" }> => message.t === "input")
      .flatMap((message) => Array.from(fromBase64(message.data)));
    expect(typed).toEqual([0xc3, 0xa9, 0x0d]);
  });

  it("pastes as typing, bracketed once the shell asks for it", async () => {
    const typed = () =>
      new TextDecoder().decode(
        Uint8Array.from(
          messages
            .filter((message): message is Extract<PageMessage, { t: "input" }> => message.t === "input")
            .flatMap((message) => Array.from(fromBase64(message.data))),
        ),
      );
    const paste = (text: string) =>
      page.evaluate((data) => window.__mono.paste(data), toBase64(textBytes(text)));
    messages.length = 0;
    await paste("ls\nél");
    await write("\x1b[?2004h");
    await page.waitForTimeout(50);
    await paste("x");
    await page.waitForTimeout(100);
    expect(typed()).toBe("ls\rél\x1b[200~x\x1b[201~");
  });

  it("keeps up with a burst of output", async () => {
    const lines = Array.from({ length: 10_000 }, (_, i) => `line ${i} \x1b[31mred\x1b[0m`);
    const started = Date.now();
    await write(lines.join("\r\n"), true);
    await rows();
    expect(Date.now() - started).toBeLessThan(3000);
    expect(await rows()).toContain("line 99");
  });

  it("scrolls history in a native scroll area, and steps aside for full-screen programs", async () => {
    const scroller = () =>
      page.evaluate(() => {
        const element = document.getElementById("scroller")!;
        return {
          top: element.scrollTop,
          room: element.scrollHeight - element.clientHeight,
          touches: getComputedStyle(element).pointerEvents,
        };
      });
    await write(Array.from({ length: 200 }, (_, i) => `row ${i}`).join("\r\n"), true);
    await rows();
    const end = await scroller();
    expect(end.room).toBeGreaterThan(0);
    expect(end.top).toBeCloseTo(end.room, -1);
    await page.evaluate(() => {
      const element = document.getElementById("scroller")!;
      element.scrollTop = 0;
      element.dispatchEvent(new Event("scroll"));
    });
    expect(await rows()).toContain("row 0row 1");
    messages.length = 0;
    await write("\x1b[?1049h");
    await page.waitForTimeout(50);
    expect((await scroller()).touches).toBe("none");
    await write("\x1b[?1049l");
    await page.waitForTimeout(50);
    expect((await scroller()).touches).toBe("auto");
    expect(messages.filter((message) => message.t === "full")).toEqual([
      { t: "full", on: true },
      { t: "full", on: false },
    ]);
  });

  it("runs history on under the key bar once scrolled back, and leaves it empty at the end", async () => {
    const under = () => page.evaluate(() => document.getElementById("overhang")!.textContent);
    const shown = messages.filter((message) => message.t === "size").length;
    await page.evaluate(() => window.__mono.inset(90));
    await page.waitForTimeout(100);
    // The screen ends above the bar, so the shell is told it has fewer rows.
    expect(messages.filter((message) => message.t === "size").length).toBeGreaterThan(shown);
    await write(Array.from({ length: 200 }, (_, i) => `line ${i}`).join("\r\n"), true);
    await rows();
    expect(await under()).toBe("");
    await page.evaluate(() => {
      const element = document.getElementById("scroller")!;
      element.scrollTop = 0;
      element.dispatchEvent(new Event("scroll"));
    });
    await rows();
    const screen = await rows();
    const last = Number(/line (\d+)\s*$/.exec(screen)![1]);
    expect(await under()).toMatch(new RegExp(`^line ${last + 1}\\s*line ${last + 2}`));
    await page.evaluate(() => window.__mono.inset(0));
  });

  it("reports a new size when the screen changes", async () => {
    const before = messages.filter((message) => message.t === "size").length;
    await page.setViewportSize({ width: 600, height: 300 });
    await page.waitForTimeout(250);
    const sizes = messages.filter((message) => message.t === "size");
    expect(sizes.length).toBeGreaterThan(before);
    expect(sizes.at(-1)).toMatchObject({ rows: expect.any(Number) });
  });
});

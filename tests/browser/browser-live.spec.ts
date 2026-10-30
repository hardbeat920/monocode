import { expect, test } from "@playwright/test";
import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";

// "Show in MonoCode" prototype against a real Chrome: MONOCODE_TEST_CHROME
// names the executable. The workspace runs with the browser module's
// commands answered here, over CDP, the way `browser/live.rs` does it.
const CHROME = process.env.MONOCODE_TEST_CHROME;
const SHOTS = process.env.MONOCODE_LIVE_SHOTS ?? "test-results";

const SHOP = `<!doctype html><meta charset=utf-8><title>Checkout · Demo shop</title>
<style>
*{box-sizing:border-box}
body{margin:0;font:15px/1.5 ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif;background:#fafafa;color:#18181b}
nav{position:sticky;top:0;display:flex;align-items:center;gap:10px;height:56px;padding:0 32px;background:#fffc;backdrop-filter:blur(8px);border-bottom:1px solid #e4e4e7;font-weight:600}
.logo{width:22px;height:22px;border-radius:7px;background:linear-gradient(135deg,#6366f1,#ec4899)}
nav .links{margin-left:auto;display:flex;gap:22px;font-weight:450;color:#71717a;font-size:14px}
.live{width:7px;height:7px;border-radius:50%;background:#22c55e;animation:p 1.2s ease-in-out infinite}
@keyframes p{50%{opacity:.25}}
main{max-width:520px;margin:40px auto;padding:0 20px}
h1{margin:0;font-size:26px;letter-spacing:-.02em}
.sub{color:#71717a;margin:4px 0 24px}
.card{background:#fff;border:1px solid #e4e4e7;border-radius:14px;padding:24px}
label{display:block;margin:0 0 6px;font-size:13px;font-weight:500;color:#3f3f46}
.field{margin-bottom:16px}
input,select{width:100%;height:40px;padding:0 12px;font:inherit;border:1px solid #d4d4d8;border-radius:9px;background:#fff;outline:none}
input:focus{border-color:#6366f1;box-shadow:0 0 0 3px #6366f126}
button{width:100%;height:42px;margin-top:4px;font:inherit;font-weight:600;border:0;border-radius:9px;background:#18181b;color:#fff;cursor:pointer}
#status{margin-top:12px;font-size:14px;color:#16a34a;min-height:21px}
h2{font-size:15px;margin:32px 0 8px}
.row{display:flex;justify-content:space-between;padding:12px 0;border-bottom:1px solid #f0f0f2;font-size:14px}
.row span:last-child{color:#71717a}
</style>
<nav><span class=logo></span>Demo shop<span class=links><span>Products</span><span>Orders</span><span>Account</span></span><span class=live></span></nav>
<main><h1>Checkout</h1><div class=sub>The agent is filling this in.</div>
<form class=card id=f onsubmit="document.getElementById('status').textContent='Order placed for '+document.getElementById('name').value;return false">
<div class=field><label for=name>Full name</label><input id=name autocomplete=off></div>
<div class=field><label for=email>Email</label><input id=email value="agent@example.com"></div>
<div class=field><label for=ship>Shipping</label><select id=ship><option>Standard (3 to 5 days)</option><option>Express (1 day)</option></select></div>
<button id=place>Place order</button><div id=status></div></form>
<h2>Order history</h2>
${Array.from({ length: 30 }, (_, i) => `<div class=row><span>Order #${1040 - i}</span><span>${i + 2} days ago</span></div>`).join("")}
</main>`;

type Pending = {
  resolve: (value: any) => void;
  reject: (error: Error) => void;
};

class Cdp {
  private next = 1;
  private pending = new Map<number, Pending>();
  listeners: ((message: any) => void)[] = [];
  constructor(private ws: WebSocket) {
    ws.onmessage = (event) => {
      const message = JSON.parse(String(event.data));
      const waiting = message.id && this.pending.get(message.id);
      if (waiting) {
        this.pending.delete(message.id);
        if (message.error)
          waiting.reject(new Error(JSON.stringify(message.error)));
        else waiting.resolve(message.result);
      } else for (const listener of this.listeners) listener(message);
    };
  }
  send(method: string, params: object = {}, sessionId?: string): Promise<any> {
    const id = this.next++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.ws.send(
        JSON.stringify({
          id,
          method,
          params,
          ...(sessionId ? { sessionId } : {}),
        }),
      );
    });
  }
}

/** The same mapping as `live::command_for`. */
function cdpInput(input: any): [string, object] {
  switch (input.kind) {
    case "mouse":
      return [
        "Input.dispatchMouseEvent",
        {
          type: input.event,
          x: input.x,
          y: input.y,
          button: input.button ?? "none",
          buttons: input.buttons ?? 0,
          clickCount: input.clickCount ?? 0,
          modifiers: input.modifiers ?? 0,
        },
      ];
    case "wheel":
      return [
        "Input.dispatchMouseEvent",
        {
          type: "mouseWheel",
          x: input.x,
          y: input.y,
          deltaX: input.deltaX,
          deltaY: input.deltaY,
          modifiers: input.modifiers ?? 0,
        },
      ];
    case "key": {
      const params: any = {
        type: input.event,
        key: input.key,
        code: input.code,
        windowsVirtualKeyCode: input.keyCode ?? 0,
        nativeVirtualKeyCode: input.keyCode ?? 0,
        modifiers: input.modifiers ?? 0,
      };
      if (input.text && input.event === "keyDown") {
        params.text = input.text;
        params.unmodifiedText = input.text;
      }
      return ["Input.dispatchKeyEvent", params];
    }
    default:
      return ["Input.insertText", { text: input.text }];
  }
}

let chrome: ChildProcess | undefined;
let profile = "";
let server: Server | undefined;
let cdp: Cdp;
let session = "";
let shopUrl = "";

test.skip(!CHROME, "MONOCODE_TEST_CHROME is not set");

test.beforeAll(async () => {
  server = createServer((_, res) => {
    res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
    res.end(SHOP);
  });
  await new Promise<void>((resolve) => server!.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  shopUrl = `http://127.0.0.1:${typeof address === "object" && address ? address.port : 0}/checkout`;

  profile = mkdtempSync(join(tmpdir(), "monocode-live-"));
  const args = [
    `--user-data-dir=${profile}`,
    "--remote-debugging-port=0",
    "--no-first-run",
    "--no-default-browser-check",
    "--window-size=1100,800",
  ];
  if (process.getuid?.() === 0) args.push("--no-sandbox");
  if (!process.env.DISPLAY) args.push("--headless=new");
  args.push("about:blank");
  chrome = spawn(CHROME!, args, { stdio: "ignore" });
  const portFile = join(profile, "DevToolsActivePort");
  for (let i = 0; i < 150 && !existsSync(portFile); i++)
    await new Promise((r) => setTimeout(r, 100));
  await new Promise((r) => setTimeout(r, 1000));
  const port = readFileSync(portFile, "utf8").split("\n")[0];
  const version = await (
    await fetch(`http://127.0.0.1:${port}/json/version`)
  ).json();
  const ws = new WebSocket(version.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    ws.onopen = resolve;
    ws.onerror = reject;
  });
  cdp = new Cdp(ws);
  // An agent tab: opened in the background, focus emulation on, as targets.rs does.
  const { targetId } = await cdp.send("Target.createTarget", {
    url: shopUrl,
    background: true,
  });
  ({ sessionId: session } = await cdp.send("Target.attachToTarget", {
    targetId,
    flatten: true,
  }));
  await cdp.send("Page.enable", {}, session);
  await cdp.send("Runtime.enable", {}, session);
  await cdp.send(
    "Emulation.setFocusEmulationEnabled",
    { enabled: true },
    session,
  );
  await new Promise((r) => setTimeout(r, 800));
});

test.afterAll(async () => {
  chrome?.kill();
  server?.close();
  await new Promise((r) => setTimeout(r, 500));
  if (profile) rmSync(profile, { recursive: true, force: true, maxRetries: 5 });
});

const pageValue = async (expression: string) =>
  (
    await cdp.send(
      "Runtime.evaluate",
      { expression, returnByValue: true },
      session,
    )
  ).result.value;

test("a background tab of the thread shows live in MonoCode and takes clicks and typing", async ({
  page,
}) => {
  test.setTimeout(150_000);
  // The running view, as `BrowserHost::live`: one at a time, stopped by its id.
  let running:
    | {
        view: string;
        stop: () => Promise<void>;
        resize: (size: any) => Promise<void>;
      }
    | undefined;
  let frames = 0;
  let pulled = 0;
  const frameTimes: number[] = [];
  // The newest picture not handed to the pane yet, and the pane's request
  // waiting for one (`browser_live_frame`), as `live::LiveControl`.
  type Picture = {
    format: string;
    data: string;
    seq: number;
    width: number;
    height: number;
  };
  let latest: Picture | null = null;
  let asking: ((picture: Picture | null) => void) | null = null;
  const inputs: string[] = [];
  const navigations: string[] = [];
  let resize: ((size: any) => Promise<void>) | undefined;
  await page.exposeFunction(
    "liveBridge",
    async (command: string, args: any) => {
      if (command === "browser_live_stop") {
        if (!args.view || running?.view === args.view) await running?.stop();
        return null;
      }
      if (command === "browser_live_frame") {
        if (latest && latest.seq > args.after) {
          const picture = latest;
          latest = null;
          pulled += 1;
          return picture;
        }
        if (!running) return null;
        return new Promise<Picture | null>((resolve) => {
          const timer = setTimeout(() => {
            if (asking === answer) asking = null;
            resolve(null);
          }, 1000);
          const answer = (picture: Picture | null) => {
            clearTimeout(timer);
            if (picture) pulled += 1;
            resolve(picture);
          };
          asking = answer;
        });
      }
      if (command === "browser_live_resize") {
        if (running?.view === args.view) await running.resize(args);
        return null;
      }
      if (command === "browser_live_navigate") {
        navigations.push(args.nav.kind);
        const nav = args.nav;
        if (nav.kind === "reload") await cdp.send("Page.reload", {}, session);
        else if (nav.kind === "go")
          // As `live::address`: a bare local host gets http.
          await cdp.send(
            "Page.navigate",
            { url: nav.url.includes("://") ? nav.url : `http://${nav.url}` },
            session,
          );
        else {
          const history = await cdp.send(
            "Page.getNavigationHistory",
            {},
            session,
          );
          const entry =
            history.entries[
              history.currentIndex + (nav.kind === "back" ? -1 : 1)
            ];
          if (entry)
            await cdp.send(
              "Page.navigateToHistoryEntry",
              { entryId: entry.id },
              session,
            );
        }
        return null;
      }
      if (command === "browser_live_input") {
        inputs.push(args.input.kind + ":" + (args.input.event ?? ""));
        const [method, params] = cdpInput(args.input);
        await cdp.send(method, params, session);
        return null;
      }
      // browser_live_start: stream until stopped, each picture acknowledged.
      return new Promise((resolve) => {
        let seq = 0;
        const listener = async (message: any) => {
          if (
            message.method !== "Page.screencastFrame" ||
            message.sessionId !== session
          )
            return;
          await cdp.send(
            "Page.screencastFrameAck",
            { sessionId: message.params.sessionId },
            session,
          );
          seq += 1;
          frames += 1;
          frameTimes.push(Date.now());
          // The pane gets the newest picture when it asks for one.
          const picture: Picture = {
            format: "jpeg",
            data: message.params.data,
            seq,
            width: message.params.metadata.deviceWidth,
            height: message.params.metadata.deviceHeight,
          };
          if (asking) {
            const answer = asking;
            asking = null;
            latest = null;
            answer(picture);
          } else {
            latest = picture;
          }
        };
        const stop = async () => {
          if (running?.view === args.view) running = undefined;
          cdp.listeners = cdp.listeners.filter((l) => l !== listener);
          // A request waiting for a picture learns there will be none.
          asking?.(null);
          asking = null;
          latest = null;
          await cdp.send("Page.stopScreencast", {}, session).catch(() => {});
          await cdp
            .send("Emulation.clearDeviceMetricsOverride", {}, session)
            .catch(() => {});
          await cdp
            .send("Emulation.setScrollbarsHidden", { hidden: false }, session)
            .catch(() => {});
          resolve("stopped");
        };
        const show = async (size: any) => {
          await cdp.send(
            "Emulation.setDeviceMetricsOverride",
            {
              width: size.width,
              height: size.height,
              deviceScaleFactor: size.scale,
              mobile: false,
            },
            session,
          );
          await cdp.send(
            "Page.startScreencast",
            {
              format: "jpeg",
              quality: 70,
              maxWidth: Math.round(size.width * size.scale),
              maxHeight: Math.round(size.height * size.scale),
            },
            session,
          );
        };
        const resize = async (size: any) => {
          await cdp.send("Page.stopScreencast", {}, session);
          await show(size);
        };
        // A new view waits for the one it replaces to hand the tab back.
        const previous = running;
        running = { view: args.view, stop, resize };
        void (previous?.stop() ?? Promise.resolve())
          .then(() =>
            cdp.send(
              "Emulation.setScrollbarsHidden",
              { hidden: true },
              session,
            ),
          )
          .then(() => {
            cdp.listeners.push(listener);
            return show(args);
          });
      });
    },
  );

  await page.setViewportSize({ width: 1440, height: 900 });
  // The redesigned layout, so its notice stays away.
  await page.addInitScript(() =>
    localStorage.setItem("monocode.insetLayout", "1"),
  );
  await page.goto(
    `/tests/browser/browser-live.html?url=${encodeURIComponent(shopUrl)}&title=${encodeURIComponent("Checkout · Demo shop")}`,
  );
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on(
    "console",
    (message) => message.type() === "error" && errors.push(message.text()),
  );
  const globe = page.getByRole("button", { name: "Browser access on" }).first();
  await globe.waitFor({ timeout: 30_000 }).catch(async (error) => {
    await page.screenshot({ path: `${SHOTS}/live-0-failed.png` });
    throw new Error(`${error}\n${errors.join("\n")}`);
  });
  await globe.click();
  await page.screenshot({ path: `${SHOTS}/live-1-menu.png` });
  await page.getByRole("button", { name: "Show in MonoCode" }).click();

  const panel = page.locator("[data-browser-live]");
  const picture = panel.locator("canvas");
  await picture.waitFor();
  // The spinner in the page's header animates, so pictures keep coming.
  const before = frames;
  await page.waitForTimeout(3000);
  const perSecond = (frames - before) / 3;
  await page.screenshot({ path: `${SHOTS}/live-2-panel.png` });

  // Click "Full name" in the picture, type, press Enter: all through the panel.
  const box = await picture.boundingBox();
  const field = JSON.parse(
    await pageValue(
      "JSON.stringify(document.getElementById('name').getBoundingClientRect())",
    ),
  );
  const viewport = JSON.parse(
    await pageValue("JSON.stringify([innerWidth, innerHeight])"),
  );
  const at = (x: number, y: number) => ({
    x: box!.x + (x / viewport[0]) * box!.width,
    y: box!.y + (y / viewport[1]) * box!.height,
  });
  const target = at(field.x + 40, field.y + field.height / 2);
  await page.mouse.click(target.x, target.y);
  await expect.poll(() => pageValue("document.activeElement.id")).toBe("name");
  await page.keyboard.type("Ognjen Ćirić šđž");
  await expect
    .poll(() => pageValue("document.getElementById('name').value"))
    .toBe("Ognjen Ćirić šđž");
  await page.keyboard.press("Enter");
  await expect
    .poll(() => pageValue("document.getElementById('status').textContent"))
    .toBe("Order placed for Ognjen Ćirić šđž");
  await page.waitForTimeout(300);
  // A burst of typed text holds the tab's pictures back for about a second
  // (typing at human speed does not, see the timing below); wait for them.
  const typedAt = frames;
  await expect
    .poll(() => frames, { timeout: 5000 })
    .toBeGreaterThan(typedAt + 20);
  await page.screenshot({ path: `${SHOTS}/live-3-typed.png` });
  // Scroll the page from the panel.
  await page.mouse.move(target.x, target.y + 200);
  await page.mouse.wheel(0, 600);
  await expect.poll(() => pageValue("scrollY")).toBeGreaterThan(100);
  await page.waitForTimeout(800);
  await page.screenshot({ path: `${SHOTS}/live-4-scrolled.png` });
  await page.mouse.wheel(0, -600);
  await expect.poll(() => pageValue("scrollY")).toBe(0);

  // Time from a key in MonoCode to the first new picture after it.
  await page.mouse.click(target.x, target.y);
  await page.keyboard.press("End");
  const delays: number[] = [];
  for (const letter of "abcdefghij") {
    await page.waitForTimeout(250);
    const pressed = Date.now();
    await page.keyboard.press(letter);
    await expect
      .poll(() => frameTimes.some((at) => at > pressed), { intervals: [5] })
      .toBe(true);
    delays.push(frameTimes.find((at) => at > pressed)! - pressed);
  }
  delays.sort((a, b) => a - b);
  console.log(
    `live panel: ${perSecond.toFixed(1)} pictures/s while the page animates; key to picture median ${delays[5]} ms, max ${delays[9]} ms; ${inputs.length} inputs; ${pulled} pictures taken by the pane`,
  );

  // The address field and Back, as in a browser.
  const address = panel.getByRole("textbox", { name: "Address" });
  await address.click();
  await address.fill(`${shopUrl.replace(/^http:\/\//, "")}?step=2`);
  await address.press("Enter");
  await expect.poll(() => pageValue("location.search")).toBe("?step=2");
  await panel.getByRole("button", { name: "Back" }).click();
  await expect.poll(() => pageValue("location.search")).toBe("");

  // The page follows the pane's size.
  const before2 = Number(await pageValue("innerWidth"));
  await page.setViewportSize({ width: 1280, height: 860 });
  await expect
    .poll(() => pageValue("innerWidth").then(Number))
    .toBeLessThan(before2);
  // The picture follows once the first one at the new size is drawn.
  await expect
    .poll(async () =>
      Math.abs(
        Number(await pageValue("innerWidth")) -
          Math.round((await picture.boundingBox())!.width),
      ),
    )
    .toBeLessThanOrEqual(1);
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${SHOTS}/live-5-resized.png` });

  // Drag the border between the thread and the browser: the page follows.
  const sash = page.locator('[data-pane-sash][aria-orientation="vertical"]');
  const border = (await sash.first().boundingBox())!;
  const widthBefore = Number(await pageValue("innerWidth"));
  await page.mouse.move(border.x + border.width / 2, border.y + 200);
  await page.mouse.down();
  await page.mouse.move(border.x + border.width / 2 + 220, border.y + 200, {
    steps: 12,
  });
  await page.mouse.up();
  await expect
    .poll(() => pageValue("innerWidth").then(Number))
    .toBeLessThan(widthBefore - 150);
  await expect
    .poll(async () =>
      Math.abs(
        Number(await pageValue("innerWidth")) -
          Math.round((await picture.boundingBox())!.width),
      ),
    )
    .toBeLessThanOrEqual(1);
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${SHOTS}/live-6-narrower.png` });

  // Move the browser pane to the left of the thread by its grip.
  const browserPane = page.locator("[data-pane-id]").filter({ has: panel });
  const thread = page.locator('[data-pane-id="workspace-session-0"]');
  const grip = (await browserPane
    .getByRole("button", { name: "Drag to reorder pane" })
    .boundingBox())!;
  const threadBox = (await thread.boundingBox())!;
  await page.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2);
  await page.mouse.down();
  await page.mouse.move(threadBox.x + 12, threadBox.y + threadBox.height / 2, {
    steps: 20,
  });
  await page.mouse.up();
  await expect
    .poll(
      async () =>
        (await browserPane.boundingBox())!.x < (await thread.boundingBox())!.x,
    )
    .toBe(true);
  // Still live on the left, at its new size: scrolling there moves the page
  // (a still page sends no pictures, so the scroll is what brings new ones).
  const movedAt = frames;
  const left = (await picture.boundingBox())!;
  await page.mouse.move(left.x + left.width / 2, left.y + left.height / 2);
  await page.mouse.wheel(0, 400);
  await expect
    .poll(() => pageValue("scrollY").then(Number))
    .toBeGreaterThan(100);
  await expect.poll(() => frames).toBeGreaterThan(movedAt);
  await expect
    .poll(async () =>
      Math.abs(
        Number(await pageValue("innerWidth")) -
          Math.round((await picture.boundingBox())!.width),
      ),
    )
    .toBeLessThanOrEqual(1);
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${SHOTS}/live-7-left.png` });
  const leftWidth = (await browserPane.boundingBox())!.width;

  // The pane's own tab (the window's title tab carries the same name).
  await page
    .locator('button[title="Close Checkout · Demo shop"]')
    .click({ timeout: 10_000 });
  await expect(panel).toHaveCount(0);

  // The next one opens where the last one was left: on the left, as wide.
  await globe.click();
  await page.getByRole("button", { name: "Show in MonoCode" }).click();
  await picture.waitFor();
  const reopened = (await browserPane.boundingBox())!;
  expect(reopened.x).toBeLessThan((await thread.boundingBox())!.x);
  expect(Math.abs(reopened.width - leftWidth)).toBeLessThan(8);
  await page
    .locator('button[title="Close Checkout · Demo shop"]')
    .click({ timeout: 10_000 });
  await expect(panel).toHaveCount(0);

  // "Show agent tabs: In MonoCode": a tab the thread gets opens on its own,
  // and the composer keeps the keyboard.
  const composerBox = page.locator(
    '[data-pane-id="workspace-session-0"] textarea, [data-pane-id="workspace-session-0"] [contenteditable="true"]',
  );
  await composerBox.first().click();
  const focusedBefore = await page.evaluate(
    () => document.activeElement?.outerHTML.slice(0, 80) ?? "",
  );
  await page.evaluate(async (url) => {
    const internals = (window as any).__TAURI_INTERNALS__;
    const state = await internals.invoke("browser_status");
    const second = {
      ...state.tabs[0],
      tab: "t_live0002",
      title: "Second tab",
      url,
    };
    (window as any).__emitTauri("monocode-browser-state", {
      ...state,
      settings: { ...state.settings, tabs_in_monocode: true },
      tabs: [...state.tabs, second],
    });
  }, shopUrl);
  await expect(panel).toHaveCount(1);
  await expect(page.locator('button[title="Close Second tab"]')).toBeVisible();
  expect((await browserPane.boundingBox())!.x).toBeLessThan(
    (await thread.boundingBox())!.x,
  );
  expect(
    await page.evaluate(
      () => document.activeElement?.outerHTML.slice(0, 80) ?? "",
    ),
  ).toBe(focusedBefore);
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${SHOTS}/live-8-auto.png` });

  expect(navigations).toEqual(["go", "back"]);
  expect(perSecond).toBeGreaterThan(5);
  // The panel reports each picture it draws, which paces the browser.
  expect(pulled).toBeGreaterThan(0);
});

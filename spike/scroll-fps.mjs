// Screencast pictures per second while the page is wheel-scrolled fast,
// sending each wheel at once ("each") or one at a time with the wheels that
// came meanwhile added up ("merged"). Also how long after the last wheel the
// page is still scrolling (the wheels the browser had yet to work through).
//
// Usage: node scroll-fps.mjs <chrome-or-edge-exe> [url]
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const [exe, url = 'https://en.wikipedia.org/wiki/Serbia'] = process.argv.slice(2);
if (!exe) { console.error('usage: node scroll-fps.mjs <exe> [url]'); process.exit(2); }
const profile = mkdtempSync(join(tmpdir(), 'mc-scrollfps-'));
const args = [`--user-data-dir=${profile}`, '--remote-debugging-port=0', '--no-first-run',
  '--no-default-browser-check', '--headless=new', '--hide-scrollbars', 'about:blank'];
if (process.getuid?.() === 0) args.push('--no-sandbox');
const child = spawn(exe, args, { stdio: 'ignore' });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let port;
for (let i = 0; i < 150 && !port; i++) {
  await sleep(100);
  const f = join(profile, 'DevToolsActivePort');
  if (existsSync(f)) port = readFileSync(f, 'utf8').split('\n')[0];
}
const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
const ws = new WebSocket(version.webSocketDebuggerUrl);
await new Promise((r) => { ws.onopen = r; });
let nextId = 1; const pending = new Map(); const listeners = [];
ws.onmessage = (m) => {
  const msg = JSON.parse(m.data);
  if (msg.id && pending.has(msg.id)) {
    const { res, rej } = pending.get(msg.id); pending.delete(msg.id);
    msg.error ? rej(new Error(JSON.stringify(msg.error))) : res(msg.result);
  } else for (const l of listeners) l(msg);
};
const send = (method, params = {}, sessionId) => new Promise((res, rej) => {
  const id = nextId++; pending.set(id, { res, rej });
  ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
});
const { targetId } = await send('Target.createTarget', { url: 'about:blank', newWindow: true });
const { sessionId: s } = await send('Target.attachToTarget', { targetId, flatten: true });
await send('Page.enable', {}, s);
await send('Emulation.setFocusEmulationEnabled', { enabled: true }, s);
const { windowId } = await send('Browser.getWindowForTarget', { targetId });
await send('Browser.setWindowBounds', { windowId, bounds: { width: 1400, height: 1390 } });
await send('Page.navigate', { url }, s);
await sleep(4000);
const m = await send('Page.getLayoutMetrics', {}, s);
const W = Math.round(m.cssVisualViewport.clientWidth), H = Math.round(m.cssVisualViewport.clientHeight);
const scrollY = async () => (await send('Runtime.evaluate', { expression: 'scrollY', returnByValue: true }, s)).result.value;

let frames = 0;
listeners.push((msg) => {
  if (msg.method !== 'Page.screencastFrame' || msg.sessionId !== s) return;
  frames++;
  send('Page.screencastFrameAck', { sessionId: msg.params.sessionId }, s).catch(() => {});
});
const size = Number(process.env.SIZE || 1);
await send('Page.startScreencast', { format: 'jpeg', quality: Number(process.env.QUALITY || 90), maxWidth: Math.round(W * size), maxHeight: Math.round(H * size) }, s);
console.log(version.Browser, `viewport ${W}x${H}`, url, `quality ${process.env.QUALITY || 90}, picture ${size}x`);

// A fast scroll: a 100 px wheel every 16 ms for 1.5 s, down then up.
async function run(mode) {
  const wheels = [];
  let owed = 0; let flying = false; let answered = Promise.resolve();
  const post = (dy) => send('Input.dispatchMouseEvent', { type: 'mouseWheel', x: W / 2, y: H / 2, deltaX: 0, deltaY: dy }, s);
  const flush = () => {
    if (flying || owed === 0) return;
    const dy = owed; owed = 0; flying = true;
    answered = post(dy).finally(() => { flying = false; flush(); });
  };
  for (const sign of [1, -1]) {
    await sleep(600);
    const before = frames; const start = performance.now();
    while (performance.now() - start < 1500) {
      const t = performance.now();
      if (mode === 'each') wheels.push(post(100 * sign).then(() => performance.now() - t));
      else if (mode === 'script') send('Runtime.evaluate', { expression: `scrollBy(0, ${100 * sign})` }, s).catch(() => {});
      else { owed += 100 * sign; flush(); }
      await sleep(16);
    }
    const fps = (frames - before) / ((performance.now() - start) / 1000);
    // Until the page stops moving: what was still on its way.
    const last = performance.now(); let y = await scrollY(); let still = 0;
    while (still < 3 && performance.now() - last < 15000) {
      await sleep(30); const now = await scrollY();
      still = now === y ? still + 1 : 0; y = now;
    }
    await answered;
    const lag = performance.now() - last - 90;
    console.log(`${mode.padEnd(6)} ${sign > 0 ? 'down' : 'up  '}: ${fps.toFixed(1)} pictures/s while scrolling, page still moving ${Math.max(0, Math.round(lag))} ms after the last wheel, ends at ${Math.round(y)}`);
  }
  if (wheels.length) {
    const took = (await Promise.all(wheels)).sort((a, b) => a - b);
    console.log(`       each wheel answered after median ${Math.round(took[took.length >> 1])} ms, slowest ${Math.round(took.at(-1))} ms`);
  }
}
for (const mode of (process.env.MODES || 'each,merged,script').split(',')) {
  await send('Runtime.evaluate', { expression: 'scrollTo(0, 0)' }, s);
  await run(mode);
}
await send('Page.stopScreencast', {}, s).catch(() => {});
ws.close(); child.kill();
await sleep(500);
try { rmSync(profile, { recursive: true, force: true }); } catch {}

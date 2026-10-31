// How far a page really scrolls for a wheel sent over CDP, and whether the
// scroll position that comes with each screencast picture matches what the
// picture shows. The live pane moves the picture ahead by the wheel's
// distance; a browser that scrolls another distance (percent-based or
// animated scrolling) makes that guess wrong.
//
// Usage: node wheel-probe.mjs <chrome-or-edge-exe> [extra switches...]
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const [exe, ...extra] = process.argv.slice(2);
if (!exe) { console.error('usage: node wheel-probe.mjs <exe> [switches...]'); process.exit(2); }
const profile = mkdtempSync(join(tmpdir(), 'mc-wheel-'));
const args = [`--user-data-dir=${profile}`, '--remote-debugging-port=0', '--no-first-run',
  '--no-default-browser-check', '--headless=new', ...extra, 'about:blank'];
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
const para = 'The quick brown fox jumps over the lazy dog. '.repeat(20);
const PAGE = 'data:text/html,' + encodeURIComponent(`<!doctype html><meta charset=utf-8><body style="font:15px system-ui;margin:20px">${
  Array.from({ length: 300 }, (_, i) => `<h2>Section ${i}</h2><p>${para}</p>`).join('')}</body>`);
const { targetId } = await send('Target.createTarget', { url: 'about:blank', newWindow: true });
const { sessionId: s } = await send('Target.attachToTarget', { targetId, flatten: true });
await send('Page.enable', {}, s);
await send('Emulation.setFocusEmulationEnabled', { enabled: true }, s);
const { windowId } = await send('Browser.getWindowForTarget', { targetId });
await send('Browser.setWindowBounds', { windowId, bounds: { width: 1000, height: 900 } });
await send('Page.navigate', { url: PAGE }, s);
await sleep(1000);
const scrollY = async () => (await send('Runtime.evaluate', { expression: 'scrollY', returnByValue: true }, s)).result.value;
console.log(version.Browser, extra.join(' ') || '(no extra switches)');

for (const delta of [40, 100, 120, 300]) {
  await send('Runtime.evaluate', { expression: 'scrollTo(0, 2000)' }, s);
  await sleep(300);
  const before = await scrollY();
  const t = performance.now();
  await send('Input.dispatchMouseEvent', { type: 'mouseWheel', x: 500, y: 450, deltaX: 0, deltaY: delta }, s);
  const trace = [];
  for (let i = 0; i < 25; i++) { await sleep(20); trace.push(Math.round(await scrollY() - before)); }
  console.log(`wheel ${delta}px: moved ${trace[trace.length - 1]}px; over time (every 20 ms) ${trace.join(' ')}; ${Math.round(performance.now() - t)} ms`);
}

// Screencast: the scroll offset each picture carries vs the page right then.
let frames = [];
listeners.push((msg) => {
  if (msg.method !== 'Page.screencastFrame' || msg.sessionId !== s) return;
  send('Page.screencastFrameAck', { sessionId: msg.params.sessionId }, s).catch(() => {});
  const m = msg.params.metadata;
  frames.push(`${Math.round(m.scrollOffsetY)}@${m.deviceWidth}x${m.deviceHeight}/${m.pageScaleFactor}`);
});
await send('Runtime.evaluate', { expression: 'scrollTo(0, 2000)' }, s);
await send('Page.startScreencast', { format: 'jpeg', quality: 80, maxWidth: 1002, maxHeight: 902 }, s);
await sleep(500);
frames = [];
for (let i = 0; i < 10; i++) {
  send('Input.dispatchMouseEvent', { type: 'mouseWheel', x: 500, y: 450, deltaX: 0, deltaY: 50 }, s).catch(() => {});
  await sleep(16);
}
await sleep(600);
console.log(`10 wheels of 50px (500px in all) from 2000, pictures' scroll@size/pageScale: ${frames.join(' ')}`);
console.log(`page scrollY at the end: ${await scrollY()}`);
await send('Page.stopScreencast', {}, s).catch(() => {});
ws.close(); child.kill();
await sleep(500);
try { rmSync(profile, { recursive: true, force: true }); } catch {}

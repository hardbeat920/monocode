// Spike: does Chrome keep painting a tab MonoCode would show in its panel
// when the browser window is moved away, minimized, or the tab is in the
// background? Counts Page.screencastFrame events per scenario and checks that
// a click and typed text sent through CDP reach the page.
//
// Usage: node screencast-spike.mjs <chrome-exe> [--flags]
//   --flags  launch with --disable-backgrounding-occluded-windows
//            --disable-renderer-backgrounding --disable-background-timer-throttling
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const exe = process.argv[2];
const withFlags = process.argv.includes('--flags');
if (!exe) { console.error('usage: node screencast-spike.mjs <chrome-exe> [--flags]'); process.exit(2); }

const profile = mkdtempSync(join(tmpdir(), 'mc-spike-'));
const args = [
  `--user-data-dir=${profile}`, '--remote-debugging-port=0',
  '--no-first-run', '--no-default-browser-check', '--window-size=1000,700',
];
if (process.getuid?.() === 0) args.push('--no-sandbox');
if (withFlags) args.push('--disable-backgrounding-occluded-windows',
  '--disable-renderer-backgrounding', '--disable-background-timer-throttling');
args.push('about:blank');
const child = spawn(exe, args, { stdio: 'ignore' });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let port;
for (let i = 0; i < 100 && !port; i++) {
  await sleep(100);
  const f = join(profile, 'DevToolsActivePort');
  if (existsSync(f)) port = readFileSync(f, 'utf8').split('\n')[0];
}
if (!port) { console.error('no DevToolsActivePort'); child.kill(); process.exit(1); }
const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();

const ws = new WebSocket(version.webSocketDebuggerUrl);
await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
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

// Animated page: a counter that changes every animation frame, a button and
// an input, so frames keep coming while the page paints.
const PAGE = 'data:text/html,' + encodeURIComponent(`<!doctype html><meta charset=utf-8>
<body style="font:20px sans-serif"><div id=n>0</div>
<button id=b style="width:200px;height:60px" onclick="this.textContent='clicked'">click me</button>
<input id=i style="width:300px;height:40px;font-size:20px">
<script>let k=0;(function f(){document.getElementById('n').textContent=++k;requestAnimationFrame(f)})()</script>`);

async function openTab(url) {
  const { targetId } = await send('Target.createTarget', { url });
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
  await send('Page.enable', {}, sessionId);
  await send('Runtime.enable', {}, sessionId);
  await sleep(500);
  return { targetId, sessionId };
}

async function countFrames(sessionId, ms) {
  let frames = 0;
  const l = (msg) => {
    if (msg.method === 'Page.screencastFrame' && msg.sessionId === sessionId) {
      frames++;
      send('Page.screencastFrameAck', { sessionId: msg.params.sessionId }, sessionId).catch(() => {});
    }
  };
  listeners.push(l);
  await send('Page.startScreencast', { format: 'jpeg', quality: 60, maxWidth: 1000, maxHeight: 700 }, sessionId);
  await sleep(ms);
  await send('Page.stopScreencast', {}, sessionId);
  listeners.splice(listeners.indexOf(l), 1);
  return frames;
}

const evalIn = async (sessionId, expr) =>
  (await send('Runtime.evaluate', { expression: expr, returnByValue: true }, sessionId)).result.value;

async function rafRate(sessionId, ms) {
  const a = await evalIn(sessionId, 'k');
  await sleep(ms);
  return Math.round(((await evalIn(sessionId, 'k')) - a) * 1000 / ms);
}

async function inputWorks(sessionId) {
  await evalIn(sessionId, "document.getElementById('b').textContent='click me';document.getElementById('i').value=''");
  const r = await evalIn(sessionId, "JSON.stringify(document.getElementById('b').getBoundingClientRect())");
  const { x, y, width, height } = JSON.parse(r);
  const cx = x + width / 2, cy = y + height / 2;
  for (const type of ['mousePressed', 'mouseReleased'])
    await send('Input.dispatchMouseEvent', { type, x: cx, y: cy, button: 'left', clickCount: 1 }, sessionId);
  await evalIn(sessionId, "document.getElementById('i').focus()");
  await send('Input.insertText', { text: 'Ćao šđž' }, sessionId);
  await sleep(200);
  const clicked = (await evalIn(sessionId, "document.getElementById('b').textContent")) === 'clicked';
  const typed = (await evalIn(sessionId, "document.getElementById('i').value")) === 'Ćao šđž';
  return { clicked, typed };
}

const SAMPLE = 3000;
const results = [];
async function scenario(name, sessionId) {
  const frames = await countFrames(sessionId, SAMPLE);
  const raf = await rafRate(sessionId, 1000);
  const input = await inputWorks(sessionId);
  const vis = await evalIn(sessionId, 'document.visibilityState');
  results.push({ scenario: name, fps: +(frames * 1000 / SAMPLE).toFixed(1), rafPerSec: raf, visibility: vis, ...input });
}

const panel = await openTab(PAGE);
const { windowId } = await send('Browser.getWindowForTarget', { targetId: panel.targetId });
const setBounds = (bounds) => send('Browser.setWindowBounds', { windowId, bounds });

await send('Target.activateTarget', { targetId: panel.targetId });
await sleep(500);
await scenario('visible, active tab', panel.sessionId);
await scenario('visible, active tab (again)', panel.sessionId);

await setBounds({ left: -4000, top: -4000 });
await sleep(1500);
await scenario('window moved off-screen', panel.sessionId);
await setBounds({ left: 50, top: 50 });
await sleep(800);

await setBounds({ windowState: 'minimized' });
await sleep(1500);
await scenario('window minimized', panel.sessionId);
await setBounds({ windowState: 'normal' });
await sleep(800);

const other = await openTab('data:text/html,<h1>other tab</h1>');
await send('Target.activateTarget', { targetId: other.targetId });
await sleep(1500);
await scenario('background tab (other tab active)', panel.sessionId);
await scenario('background tab (again)', panel.sessionId);

await send('Emulation.setFocusEmulationEnabled', { enabled: true }, panel.sessionId);
await scenario('background tab + focus emulation', panel.sessionId);
await send('Emulation.setFocusEmulationEnabled', { enabled: false }, panel.sessionId);
await sleep(500);
await scenario('background tab, focus emulation off again', panel.sessionId);
await send('Emulation.setFocusEmulationEnabled', { enabled: true }, panel.sessionId);
await setBounds({ left: -4000, top: -4000 });
await sleep(1500);
await scenario('off-screen window + background tab + focus emu', panel.sessionId);
await setBounds({ left: 50, top: 50 });
await sleep(800);
await setBounds({ windowState: 'minimized' });
await sleep(1500);
await scenario('minimized window + background tab + focus emu', panel.sessionId);

console.log(JSON.stringify({ browser: version.Browser, platform: process.platform, flags: withFlags, results }, null, 2));
await send('Browser.close').catch(() => {});
ws.close();
await sleep(500);
child.kill();
try { rmSync(profile, { recursive: true, force: true }); } catch {}
process.exit(0);

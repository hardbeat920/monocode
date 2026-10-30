// Measures what the browser itself delivers for the live pane, with
// MonoCode out of the picture: screencast pictures per second while a page
// scrolls, their size, and how long a wheel takes to come back as a picture.
//
// Usage: node screencast-bench.mjs <chrome-or-edge-exe> [scale] [url]
//   scale  device pixel ratio, as MonoCode passes it (default 1.25)
//   url    page to scroll (default: a long generated article)
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const [exe, scaleArg, urlArg] = process.argv.slice(2);
if (!exe) { console.error('usage: node screencast-bench.mjs <exe> [scale] [url]'); process.exit(2); }
const scale = Number(scaleArg ?? 1.25);
const profile = mkdtempSync(join(tmpdir(), 'mc-bench-'));
const args = [`--user-data-dir=${profile}`, '--remote-debugging-port=0', '--no-first-run',
  '--no-default-browser-check', '--headless=new', `--force-device-scale-factor=${scale}`, 'about:blank'];
if (process.getuid?.() === 0) args.push('--no-sandbox');
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

const para = 'The quick brown fox jumps over the lazy dog while the article keeps going with ordinary text. '.repeat(8);
const PAGE = urlArg ?? 'data:text/html,' + encodeURIComponent(`<!doctype html><meta charset=utf-8>
<body style="font:15px/1.6 system-ui;margin:24px auto;max-width:860px">${
  Array.from({ length: 200 }, (_, i) => `<h2>Section ${i}</h2><p>${para}</p>`).join('')}</body>`);
const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
const { sessionId: s } = await send('Target.attachToTarget', { targetId, flatten: true });
await send('Page.enable', {}, s);
await send('Emulation.setFocusEmulationEnabled', { enabled: true }, s);
await send('Emulation.setDeviceMetricsOverride', { width: 1000, height: 900, deviceScaleFactor: scale, mobile: false }, s);
await send('Page.navigate', { url: PAGE }, s);
await sleep(urlArg ? 4000 : 800);

let frames = 0, bytes = 0, dims = '';
let waitingSince = 0; const wheelToPicture = [];
listeners.push((msg) => {
  if (msg.method !== 'Page.screencastFrame' || msg.sessionId !== s) return;
  send('Page.screencastFrameAck', { sessionId: msg.params.sessionId }, s).catch(() => {});
  frames += 1; bytes += msg.params.data.length;
  if (!dims) {
    const b = Buffer.from(msg.params.data, 'base64'); let i = 2;
    while (i < b.length) { const m = b[i + 1]; if (m >= 0xc0 && m <= 0xc2) { dims = `${b.readUInt16BE(i + 7)}x${b.readUInt16BE(i + 5)}`; break; } i += 2 + b.readUInt16BE(i + 2); }
  }
  if (waitingSince) { wheelToPicture.push(performance.now() - waitingSince); waitingSince = 0; }
});
await send('Page.startScreencast', { format: 'jpeg', quality: 90, maxWidth: Math.ceil(1000 * scale) + 2, maxHeight: Math.ceil(900 * scale) + 2 }, s);
await sleep(500);
frames = 0; bytes = 0;
const wheelRoundTrip = [];
const started = performance.now();
while (performance.now() - started < 5000) {
  const t = performance.now();
  waitingSince = t;
  await send('Input.dispatchMouseEvent', { type: 'mouseWheel', x: 500, y: 450, deltaX: 0, deltaY: 60 }, s);
  wheelRoundTrip.push(performance.now() - t);
  await sleep(16);
}
const secs = (performance.now() - started) / 1000;
const med = (a) => a.length ? [...a].sort((x, y) => x - y)[a.length >> 1].toFixed(0) : '-';
const max = (a) => a.length ? Math.max(...a).toFixed(0) : '-';
console.log(`${version.Browser} headless, scale ${scale}, 1000x900 css`);
console.log(`pictures while scrolling: ${(frames / secs).toFixed(1)}/s, ${dims} px, ${(bytes / frames / 1024).toFixed(0)} KB base64 each`);
console.log(`wheel round trip: median ${med(wheelRoundTrip)} ms, max ${max(wheelRoundTrip)} ms`);
console.log(`wheel to next picture: median ${med(wheelToPicture)} ms, max ${max(wheelToPicture)} ms`);
await send('Page.stopScreencast', {}, s).catch(() => {});
ws.close(); child.kill();
await sleep(500);
try { rmSync(profile, { recursive: true, force: true }); } catch {}

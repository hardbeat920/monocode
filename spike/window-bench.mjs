// Which way of running the browser gives the live pane the freshest
// pictures: headless as today, headless on the GPU, or a real window kept
// out of sight. Each variant scrolls the same page for a few seconds and
// reports pictures per second, how old they are when they get here, and how
// long a wheel takes to show up as a picture.
//
// Usage: node window-bench.mjs <chrome-or-edge-exe> [scale] [url] [variant...]
//   scale    device pixel ratio for the headless variants (default 1)
//   url      page to scroll, or "-" for a long generated article
//   variant  any of the names below (default: all)
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const [exe, scaleArg, urlArg, ...only] = process.argv.slice(2);
if (!exe) { console.error('usage: node window-bench.mjs <exe> [scale] [url] [variant...]'); process.exit(2); }
const scale = Number(scaleArg ?? 1);
const W = 1000, H = 900;
const angle = { win32: 'd3d11', darwin: 'metal', linux: 'gl' }[process.platform] ?? 'default';
const quiet = ['--disable-backgrounding-occluded-windows', '--disable-renderer-backgrounding',
  '--disable-background-timer-throttling'];
const dsf = scale > 1 ? [`--force-device-scale-factor=${scale}`] : [];
const VARIANTS = {
  // Today's MonoCode: headless, the pane's size emulated.
  'headless-emulated': { args: ['--headless=new', ...dsf], emulate: true },
  // Headless, the tab's own window sized to the pane: no emulation.
  'headless-window': { args: ['--headless=new', ...dsf] },
  'headless-gpu': { args: ['--headless=new', ...dsf, `--use-angle=${angle}`, '--enable-gpu', '--ignore-gpu-blocklist'] },
  'headless-gpu-raster': { args: ['--headless=new', ...dsf, `--use-angle=${angle}`, '--enable-gpu',
    '--ignore-gpu-blocklist', '--enable-gpu-rasterization', '--enable-zero-copy'] },
  // A real window far off the screen.
  'window-offscreen': { args: ['--window-position=-32000,-32000', ...quiet], offscreen: true },
  // A real window, minimized right after start.
  'window-minimized': { args: ['--window-position=-32000,-32000', ...quiet], offscreen: true, minimize: true },
  // A real window on the screen: the best the browser can do.
  'window-visible': { args: ['--window-position=40,40', ...quiet] },
};

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const med = (a) => a.length ? [...a].sort((x, y) => x - y)[a.length >> 1].toFixed(0) : '-';
const p90 = (a) => a.length ? [...a].sort((x, y) => x - y)[Math.floor(a.length * 0.9)].toFixed(0) : '-';
const max = (a) => a.length ? Math.max(...a).toFixed(0) : '-';
const para = 'The quick brown fox jumps over the lazy dog while the article keeps going with ordinary text. '.repeat(8);
const PAGE = urlArg && urlArg !== '-' ? urlArg : 'data:text/html,' + encodeURIComponent(`<!doctype html><meta charset=utf-8>
<body style="font:15px/1.6 system-ui;margin:24px auto;max-width:860px">${
  Array.from({ length: 200 }, (_, i) => `<h2>Section ${i}</h2><p>${para}</p>`).join('')}</body>`);

async function bench(name, variant) {
  const profile = mkdtempSync(join(tmpdir(), 'mc-wbench-'));
  const args = [`--user-data-dir=${profile}`, '--remote-debugging-port=0', '--no-first-run',
    '--no-default-browser-check', `--window-size=${W},${H}`, ...variant.args, 'about:blank'];
  if (process.getuid?.() === 0) args.push('--no-sandbox');
  const child = spawn(exe, args, { stdio: 'ignore' });
  let port;
  for (let i = 0; i < 150 && !port; i++) {
    await sleep(100);
    const f = join(profile, 'DevToolsActivePort');
    if (existsSync(f)) port = readFileSync(f, 'utf8').split('\n')[0];
  }
  if (!port) { child.kill(); return { name, error: 'no DevToolsActivePort' }; }
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
  const result = { name, browser: version.Browser };
  try {
    const gpu = await send('SystemInfo.getInfo').catch(() => null);
    const status = gpu?.gpu?.featureStatus ?? {};
    result.gpu = `compositing ${status.gpu_compositing ?? '?'}, raster ${status.rasterization ?? '?'}`;
    const { targetId } = await send('Target.createTarget', { url: 'about:blank', newWindow: true });
    const { sessionId: s } = await send('Target.attachToTarget', { targetId, flatten: true });
    await send('Page.enable', {}, s);
    await send('Runtime.enable', {}, s);
    await send('Emulation.setFocusEmulationEnabled', { enabled: true }, s);
    const { windowId } = await send('Browser.getWindowForTarget', { targetId });
    const inner = async () => (await send('Runtime.evaluate',
      { expression: '[innerWidth, innerHeight, devicePixelRatio, document.visibilityState]', returnByValue: true }, s)).result.value;
    if (variant.emulate) {
      await send('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: scale, mobile: false }, s);
    } else {
      // The window's frame (title bar, borders) comes on top of the page.
      await send('Browser.setWindowBounds', { windowId, bounds: { width: W, height: H } });
      await sleep(200);
      const [iw, ih] = await inner();
      await send('Browser.setWindowBounds', { windowId, bounds: { width: W + (W - iw), height: H + (H - ih) } });
      if (variant.offscreen) {
        await send('Browser.setWindowBounds', { windowId, bounds: { left: -32000, top: -32000 } }).catch(() => {});
      }
    }
    if (variant.minimize) await send('Browser.setWindowBounds', { windowId, bounds: { windowState: 'minimized' } });
    // Close the start page so the agent tab is the only one.
    for (const t of (await send('Target.getTargets')).targetInfos) {
      if (t.type === 'page' && t.targetId !== targetId) await send('Target.closeTarget', { targetId: t.targetId }).catch(() => {});
    }
    await send('Page.navigate', { url: PAGE }, s);
    await sleep(urlArg && urlArg !== '-' ? 4000 : 1000);
    const [iw, ih, dpr, visibility] = await inner();
    const bounds = (await send('Browser.getWindowBounds', { windowId })).bounds;
    result.viewport = `${iw}x${ih} @${dpr} ${visibility}`;
    result.window = `${bounds.windowState} at ${bounds.left},${bounds.top} ${bounds.width}x${bounds.height}`;

    let frames = 0; let dims = '';
    const ages = []; let waitingSince = 0; const wheelToPicture = [];
    listeners.push((msg) => {
      if (msg.method !== 'Page.screencastFrame' || msg.sessionId !== s) return;
      send('Page.screencastFrameAck', { sessionId: msg.params.sessionId }, s).catch(() => {});
      frames += 1;
      const at = msg.params.metadata?.timestamp;
      if (at) ages.push(Date.now() - at * 1000);
      if (!dims) {
        const b = Buffer.from(msg.params.data, 'base64'); let i = 2;
        while (i + 8 < b.length) { const m = b[i + 1]; if (m >= 0xc0 && m <= 0xc2) { dims = `${b.readUInt16BE(i + 7)}x${b.readUInt16BE(i + 5)}`; break; } i += 2 + b.readUInt16BE(i + 2); }
      }
      if (waitingSince) { wheelToPicture.push(performance.now() - waitingSince); waitingSince = 0; }
    });
    const px = (v) => Math.ceil(v * (variant.emulate ? scale : dpr)) + 2;
    await send('Page.startScreencast', { format: 'jpeg', quality: 90, maxWidth: px(W), maxHeight: px(H) }, s);
    await sleep(600);
    frames = 0; ages.length = 0;
    const started = performance.now();
    while (performance.now() - started < 5000) {
      if (!waitingSince) waitingSince = performance.now();
      // Sent without waiting for the answer, as the pane does.
      send('Input.dispatchMouseEvent', { type: 'mouseWheel', x: 500, y: 450, deltaX: 0, deltaY: 50 }, s).catch(() => {});
      await sleep(16);
    }
    const secs = (performance.now() - started) / 1000;
    result.fps = (frames / secs).toFixed(1);
    result.pixels = dims;
    result.age = `median ${med(ages)}, p90 ${p90(ages)}, max ${max(ages)}`;
    result.wheel = `median ${med(wheelToPicture)}, p90 ${p90(wheelToPicture)}`;
    await send('Page.stopScreencast', {}, s).catch(() => {});
  } catch (error) {
    result.error = String(error.message ?? error);
  }
  ws.close(); child.kill();
  await sleep(800);
  try { rmSync(profile, { recursive: true, force: true }); } catch {}
  return result;
}

const names = only.length ? only : Object.keys(VARIANTS);
for (const name of names) {
  if (!VARIANTS[name]) { console.error(`unknown variant ${name}`); continue; }
  const r = await bench(name, VARIANTS[name]);
  console.log(`\n== ${r.name}${r.browser ? ` (${r.browser})` : ''}`);
  if (r.error) console.log(`   error: ${r.error}`);
  for (const key of ['gpu', 'window', 'viewport', 'pixels', 'fps', 'age', 'wheel']) {
    if (r[key] !== undefined) console.log(`   ${key.padEnd(9)}${r[key]}${key === 'age' || key === 'wheel' ? ' ms' : ''}`);
  }
}

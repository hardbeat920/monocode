import { afterAll, beforeAll, expect, it, vi } from "vitest";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { HostChildBackend } from "./child-backend";
import { HostStore } from "./store";
import { HostEngine } from "./engine";
import { hostProviders } from "./providers";
import {
  acquireHarnessBridge,
  configureChildBackend,
} from "../src/integrations/harness/core/child";

const fixture = `const http = require('node:http');
const args = process.argv.slice(2);
if (args.includes('--version')) { console.log('1.20.0'); process.exit(0); }
if (args.join(' ') === 'debug paths') { console.log('data       ' + process.cwd() + '/fixture-data'); process.exit(0); }
if (args.join(' ') === 'agent list') { console.log('build (primary)\\n' + JSON.stringify([{permission:'*',pattern:'*',action:'allow'}])); process.exit(0); }
if (args[0] === 'models') { console.log('openai/fixture-model\\n' + JSON.stringify({id:'fixture-model',name:'Fixture model'})); process.exit(0); }
const port = Number(args.find(arg => arg.startsWith('--port='))?.slice(7));
const subscribers = new Set();
const session = {id: 'fixture_open', directory: process.cwd()};
const config = JSON.parse(process.env.OPENCODE_CONFIG_CONTENT || '{}');
const agentPermissions = permission => Object.entries(permission).flatMap(([key, value]) =>
  typeof value === 'string'
    ? [{permission:key,pattern:'*',action:value}]
    : Object.entries(value).map(([pattern, action]) => ({permission:key,pattern,action})));
let messages = [];
let status = 'idle';
const server = http.createServer(async (req, res) => {
  if (req.url?.startsWith('/event')) {
    res.writeHead(200, {'Content-Type': 'text/event-stream'});
    res.flushHeaders();
    subscribers.add(res);
    req.on('close', () => subscribers.delete(res));
    return;
  }
  const path = new URL(req.url, 'http://127.0.0.1').pathname;
  if (path === '/config' || path === '/agent') {
    res.writeHead(200, {'Content-Type': 'application/json'});
    res.end(JSON.stringify(path === '/config' ? config : Object.entries(config.agent || {}).map(([name, agent]) =>
      ({name,mode:['build','plan'].includes(name)?'primary':'subagent',permission:agentPermissions(agent.permission)}))));
    return;
  }
  if (path === '/session/status') {
    res.writeHead(200, {'Content-Type': 'application/json'});
    res.end(JSON.stringify({fixture_open: {type: status}}));
    return;
  }
  if (path === '/session' || path === '/session/fixture_open') {
    res.writeHead(200, {'Content-Type': 'application/json'});
    res.end(JSON.stringify(session));
    return;
  }
  if (path === '/session/fixture_open/message') {
    res.writeHead(200, {'Content-Type': 'application/json'});
    res.end(JSON.stringify(req.method === 'POST'
      ? {info: {id:'title',role:'assistant',finish:'stop'},parts:[{id:'title-text',type:'text',text:'Fixture title'}]}
      : messages));
    return;
  }
  if (path === '/session/fixture_open/prompt_async') {
    let body = ''; for await (const chunk of req) body += chunk;
    const input = JSON.parse(body);
    const user = {sessionID: 'fixture_open', id: input.messageID, role: 'user',time:{created:Date.now()}};
    const assistant = {sessionID: 'fixture_open', id: 'msg', parentID: input.messageID, role: 'assistant', finish: 'stop',time:{created:Date.now(),completed:Date.now()+1}};
    const part = {sessionID:'fixture_open',id:'part',messageID:'msg',type:'text',text:'Headless OpenCode completed',time:{end:Date.now()+1}};
    messages = [{info: user, parts: input.parts}, {info: assistant, parts: [part]}];
    status = 'busy';
    res.writeHead(204); res.end();
    setTimeout(() => {
      status = 'idle';
      const events = [
        {type: 'message.updated', properties: {info: user}},
        {type: 'message.updated', properties: {info: assistant}},
        {type: 'message.part.updated', properties: {part}},
        {type: 'session.status', properties: {sessionID: 'fixture_open', status: {type: 'idle'}}},
      ];
      for (const subscriber of subscribers) for (const event of events)
        subscriber.write('data: ' + JSON.stringify(event) + '\\n\\n');
    }, 30);
    return;
  }
  res.writeHead(204); res.end();
});
server.listen(port, '127.0.0.1', () => console.log('opencode server listening on http://127.0.0.1:' + port));
`;

let directory: string;
let store: HostStore;
let engine: HostEngine;
let backend: HostChildBackend;
let release: () => void;

beforeAll(async () => {
  directory = mkdtempSync(join(tmpdir(), "monocode-opencode-transport-"));
  const binary = join(directory, "opencode.cjs");
  writeFileSync(binary, fixture);
  backend = new HostChildBackend({ opencode: binary });
  configureChildBackend(backend);
  release = await acquireHarnessBridge();
  store = new HostStore(join(directory, "host.db"));
  engine = new HostEngine(store, hostProviders);
});

afterAll(async () => {
  await engine?.close();
  await backend?.close();
  release?.();
  store?.close();
  if (directory) rmSync(directory, { recursive: true, force: true });
});

it("runs an OpenCode session through the host HTTP and SSE bridge", async () => {
  const project = await engine.openProject(directory);
  const { sessionId } = engine.command({
    type: "create",
    commandId: "create-opencode",
    projectId: project.id,
    harness: "opencode",
    model: "opencode:openai/fixture-model",
    runtimeMode: "supervised",
  });
  engine.command({
    type: "send",
    commandId: "send-opencode",
    sessionId,
    text: "hello",
  });
  await vi.waitFor(() => expect(store.session(sessionId).status).toBe("idle"), {
    timeout: 8_000,
  });
  const state = store.session(sessionId).session;
  expect(state.blocks.at(-1)?.text).toContain("Headless OpenCode completed");
  expect(state.providerSessionId).toBe("fixture_open");
});

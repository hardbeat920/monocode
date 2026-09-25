import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { mkdtempSync, writeFileSync, rmSync, realpathSync } from "node:fs";
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

// Real subprocesses exercise framing, startup, stdout delivery and teardown
// through the existing production adapters without contacting a paid model.
const fixture = `#!/usr/bin/env node
const readline = require('node:readline');
const send = value => process.stdout.write(JSON.stringify(value) + '\\n');
readline.createInterface({input: process.stdin}).on('line', line => {
  const request = JSON.parse(line);
  if (request.method === 'initialize') send({id: request.id, result: {}});
  if (request.method === 'thread/start' || request.method === 'thread/resume') send({id: request.id, result: {thread: {id: 'fixture-thread'}}});
  if (request.method === 'turn/start') {
    send({id: request.id, result: {turn: {id: 'fixture-turn'}}});
    setTimeout(() => {
      send({method: 'item/agentMessage/delta', params: {threadId: 'fixture-thread', turnId: 'fixture-turn', itemId: 'message', delta: 'Headless Codex completed'}});
      send({method: 'turn/completed', params: {threadId: 'fixture-thread', turn: {id: 'fixture-turn', status: 'completed'}}});
    }, 30);
  }
  if (request.type === 'control_request' && request.request.subtype === 'initialize') {
    send({type: 'system', subtype: 'init', session_id: 'fixture-claude'});
    send({type: 'control_response', response: {subtype: 'success', request_id: request.request_id}});
  }
  if (request.type === 'user') setTimeout(() => {
    send({type: 'assistant', session_id: 'fixture-claude', message: {content: [{type: 'text', text: 'Headless Claude completed'}]}});
    send({type: 'result', subtype: 'success', session_id: 'fixture-claude'});
  }, 30);
});
`;

describe("existing providers over headless process I/O", () => {
  let directory: string;
  let backend: HostChildBackend;
  let release: () => void;
  let store: HostStore;
  let engine: HostEngine;
  beforeAll(async () => {
    directory = realpathSync(
      mkdtempSync(join(tmpdir(), "monocode-provider-test-")),
    );
    const binary = join(directory, "provider.cjs");
    writeFileSync(binary, fixture, { mode: 0o700 });
    backend = new HostChildBackend({ codex: binary, claude: binary });
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

  it.each(["codex", "claude"] as const)(
    "completes and resumes %s with no React or Tauri process",
    async (harness) => {
      const project = await engine.openProject(directory);
      const { sessionId } = engine.command({
        type: "create",
        commandId: `create-${harness}`,
        projectId: project.id,
        harness,
        model: `${harness}:test`,
        runtimeMode: "supervised",
      });
      for (let turn = 0; turn < 2; turn++) {
        engine.command({
          type: "send",
          commandId: `${harness}-${turn}`,
          sessionId,
          text: "hello",
        });
        await vi.waitFor(
          () => expect(store.session(sessionId).status).toBe("idle"),
          { timeout: 4_000 },
        );
        const state = store.session(sessionId).session;
        expect(
          state.blocks.filter((block) => block.role === "assistant"),
        ).toHaveLength(turn + 1);
        expect(state.blocks.at(-1)?.text).toContain("completed");
        expect(state.providerSessionId).toBeTruthy();
      }
    },
  );
});

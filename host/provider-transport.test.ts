import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import {
  mkdtempSync,
  readFileSync,
  writeFileSync,
  rmSync,
  realpathSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { HostChildBackend } from "./child-backend";
import { HostStore } from "./store";
import { HostEngine } from "./engine";
import { hostProviders } from "./providers";
import { discoverCodexModels } from "../src/integrations/harness/providers/codex/codexCatalog";
import { discoverClaudeModels } from "../src/integrations/harness/providers/claude/claudeCatalog";
import { discoverPiModels, discoverOmpModels } from "../src/integrations/harness/providers/pi/piCatalog";
import {
  acquireHarnessBridge,
  configureChildBackend,
} from "../src/integrations/harness/core/child";

// Real subprocesses exercise framing, startup, stdout delivery and teardown
// through the existing production adapters without contacting a paid model.
const fixture = `#!/usr/bin/env node
const readline = require('node:readline');
const send = value => process.stdout.write(JSON.stringify(value) + '\\n');
// Records what each turn actually received, so tests can prove that settings
// applied between turns reach the provider.
const record = value => require('node:fs').appendFileSync(require('node:path').join(__dirname, 'calls.log'), JSON.stringify(value) + '\\n');
if (!process.argv.includes('app-server')) record({claudeArgs: process.argv.slice(2)});
let pendingPermissionPrompt;
readline.createInterface({input: process.stdin}).on('line', line => {
  const request = JSON.parse(line);
  if (request.jsonrpc === '2.0') {
    if (request.id == null) return;
    if (request.id === 'droid-host-permission' && request.result?.outcome) {
      record({permissionReplyId: request.id, permissionOutcome: request.result.outcome});
      send({jsonrpc: '2.0', id: pendingPermissionPrompt, result: {stopReason: 'end_turn'}});
      return;
    }
    if (request.method === 'session/prompt') {
      if (request.params.prompt.some(block => block.type === 'text' && block.text === 'request-permission')) {
        pendingPermissionPrompt = request.id;
        send({jsonrpc: '2.0', id: 'droid-host-permission', method: 'session/request_permission', params: {sessionId: 'fixture_acp', toolCall: {toolCallId: 'fixture-edit', kind: 'edit', title: 'Edit a fixture'}, options: [{optionId: 'proceed_once', kind: 'allow_once', name: 'Allow'}, {optionId: 'cancel_once', kind: 'reject_once', name: 'Deny'}]}});
        return;
      }
      send({jsonrpc: '2.0', method: 'session/update', params: {sessionId: 'fixture_acp', update: {sessionUpdate: 'agent_message_chunk', content: {type: 'text', text: 'Headless ACP completed'}}}});
      setTimeout(() => send({jsonrpc: '2.0', id: request.id, result: {stopReason: 'end_turn'}}), 30);
    } else {
      send({jsonrpc: '2.0', id: request.id, result: request.method === 'session/new' || request.method === 'session/load' || request.method === 'session/resume' ? {sessionId: 'fixture_acp', configOptions: []} : {}});
    }
    return;
  }
  if (request.method === 'initialize') send({id: request.id, result: {}});
  if (request.method === 'account/read') send({id: request.id, result: {account: {type: 'fixture'}, requiresOpenaiAuth: false}});
  if (request.method === 'model/list') send({id: request.id, result: {data: [{model: 'fixture-model', displayName: 'Fixture model', supportedReasoningEfforts: ['low', 'high']}], nextCursor: null}});
  if (request.method === 'thread/start' || request.method === 'thread/resume') send({id: request.id, result: {thread: {id: 'fixture-thread'}}});
  if (request.method === 'turn/start') {
    record({codexEffort: request.params.effort ?? null});
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
  if (request.type === 'control_request' && request.request.subtype === 'list_models') send({type: 'control_response', response: {subtype: 'success', request_id: request.request_id, response: {models: [{value: 'claude-fixture-model', resolvedModel: 'claude-fixture-model', displayName: 'Fixture Claude'}]}}});
  if (request.type === 'user') setTimeout(() => {
    send({type: 'assistant', session_id: 'fixture-claude', message: {content: [{type: 'text', text: 'Headless Claude completed'}]}});
    send({type: 'result', subtype: 'success', session_id: 'fixture-claude'});
  }, 30);
  if (request.type === 'get_state') send({type: 'response', id: request.id, command: 'get_state', success: true, data: {sessionId: 'fixture_pi', model: {provider: 'openai', id: 'fixture-model', contextWindow: 100000}}});
  if (request.type === 'get_session_stats') send({type: 'response', id: request.id, command: 'get_session_stats', success: true, data: {contextWindow: 100000}});
  if (request.type === 'get_available_models') send({type: 'response', id: request.id, command: 'get_available_models', success: true, data: {models: [{provider: 'openai', id: 'fixture-model', name: 'Fixture model'}]}});
  if (request.type === 'prompt') {
    send({type: 'response', id: request.id, command: 'prompt', success: true, data: {}});
    setTimeout(() => {
      send({type: 'message_update', assistantMessageEvent: {type: 'text_delta', delta: 'Headless Pi completed'}});
      send({type: 'agent_settled'});
    }, 30);
  }
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
    backend = new HostChildBackend({
      codex: binary,
      claude: binary,
      pi: binary,
      omp: binary,
      cursor: binary,
      grok: binary,
      fx: binary,
      hermes: binary,
      droid: binary,
      antigravity: binary,
    });
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

  it("discovers host models in parallel without probe process collisions", async () => {
    const [codexA, codexB, claudeA, claudeB, piA, piB, ompA, ompB] = await Promise.all([
      discoverCodexModels(directory),
      discoverCodexModels(directory),
      discoverClaudeModels(directory),
      discoverClaudeModels(directory),
      discoverPiModels(directory),
      discoverPiModels(directory),
      discoverOmpModels(directory),
      discoverOmpModels(directory),
    ]);
    expect(codexA).toEqual(codexB);
    expect(codexA[0]).toMatchObject({ id: "codex:fixture-model" });
    expect(claudeA).toEqual(claudeB);
    expect(claudeA[0]).toMatchObject({ nativeId: "claude-fixture-model" });
    expect(piA).toEqual(piB);
    expect(piA[0]).toMatchObject({ id: "pi:openai/fixture-model" });
    expect(ompA).toEqual(ompB);
    expect(ompA[0]).toMatchObject({ id: "omp:openai/fixture-model" });
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

  it.each(["pi", "omp"] as const)(
    "completes and resumes %s over the host RPC transport",
    async (harness) => {
      const project = await engine.openProject(directory);
      const { sessionId } = engine.command({
        type: "create",
        commandId: `create-${harness}`,
        projectId: project.id,
        harness,
        model: `${harness}:default`,
        runtimeMode: "supervised",
      });
      for (let turn = 0; turn < 2; turn++) {
        engine.command({
          type: "send",
          commandId: `${harness}-send-${turn}`,
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
        expect(state.blocks.at(-1)?.text).toContain("Headless Pi completed");
        expect(state.providerSessionId).toBe("fixture_pi");
      }
    },
  );

  it.each(["cursor", "grok", "fx", "hermes", "droid", "antigravity"] as const)(
    "completes a %s turn over the headless ACP transport",
    async (harness) => {
      const project = await engine.openProject(directory);
      const { sessionId } = engine.command({
        type: "create",
        commandId: `create-${harness}`,
        projectId: project.id,
        harness,
        model: `${harness}:default`,
        runtimeMode: "supervised",
      });
      engine.command({
        type: "send",
        commandId: `${harness}-send`,
        sessionId,
        text: "hello",
      });
      await vi.waitFor(
        () => expect(store.session(sessionId).status).toBe("idle"),
        { timeout: 4_000 },
      );
      const state = store.session(sessionId).session;
      expect(state.blocks.at(-1)?.text).toContain("Headless ACP completed");
      expect(state.providerSessionId).toBe("fixture_acp");
    },
  );

  it.each(["allow", "deny"] as const)(
    "round-trips a string Droid permission ID through host %s approval",
    async (decision) => {
      const project = await engine.openProject(directory);
      const { sessionId } = engine.command({
        type: "create",
        commandId: `droid-permission-create-${decision}`,
        projectId: project.id,
        harness: "droid",
        model: "droid:default",
        runtimeMode: "supervised",
      });
      engine.command({
        type: "send",
        commandId: `droid-permission-send-${decision}`,
        sessionId,
        text: "request-permission",
      });
      await vi.waitFor(
        () =>
          expect(
            store.session(sessionId).session.blocks.some(
              (block) => block.approval && !block.approval.decided,
            ),
          ).toBe(true),
        { timeout: 4_000 },
      );
      const pending = store.session(sessionId);
      const requestId = pending.session.blocks.find(
        (block) => block.approval && !block.approval.decided,
      )!.approval!.requestId;
      const log = join(directory, "calls.log");
      writeFileSync(log, "");
      engine.command({
        type: "approve",
        commandId: `droid-permission-approve-${decision}`,
        sessionId,
        runId: pending.runId!,
        requestId,
        decision,
      });
      await vi.waitFor(
        () => expect(store.session(sessionId).status).toBe("idle"),
        { timeout: 4_000 },
      );
      const calls = readFileSync(log, "utf8")
        .trim()
        .split("\n")
        .map((line) => JSON.parse(line));
      expect(calls.find((call) => call.permissionReplyId)).toEqual({
        permissionReplyId: "droid-host-permission",
        permissionOutcome: {
          outcome: "selected",
          optionId: decision === "allow" ? "proceed_once" : "cancel_once",
        },
      });
    },
  );

  it.each([
    ["codex", "reasoningEffort"],
    ["claude", "effort"],
  ] as const)(
    "uses %s reasoning effort applied between turns on the next turn",
    async (harness, setting) => {
      const log = join(directory, "calls.log");
      const project = await engine.openProject(directory);
      const { sessionId } = engine.command({
        type: "create",
        commandId: `effort-create-${harness}`,
        projectId: project.id,
        harness,
        model: `${harness}:test`,
        modelSettings: { [setting]: "low" },
        runtimeMode: "supervised",
      });
      const efforts: Array<string | null> = [];
      for (const [turn, effort] of ["low", "high"].entries()) {
        if (turn)
          engine.command({
            type: "configure",
            commandId: `effort-configure-${harness}`,
            sessionId,
            model: `${harness}:test`,
            modelSettings: { [setting]: effort },
            runtimeMode: "supervised",
          });
        writeFileSync(log, "");
        engine.command({
          type: "send",
          commandId: `effort-${harness}-${turn}`,
          sessionId,
          text: "hello",
        });
        await vi.waitFor(
          () => expect(store.session(sessionId).status).toBe("idle"),
          { timeout: 4_000 },
        );
        const calls = readFileSync(log, "utf8")
          .trim()
          .split("\n")
          .map((line) => JSON.parse(line));
        if (harness === "codex")
          efforts.push(calls.find((call) => "codexEffort" in call).codexEffort);
        else {
          const args: string[] = calls.find(
            (call) => call.claudeArgs,
          ).claudeArgs;
          efforts.push(args[args.indexOf("--effort") + 1] ?? null);
        }
      }
      expect(efforts).toEqual(["low", "high"]);
      expect(store.session(sessionId).session.modelSettings).toEqual({
        [setting]: "high",
      });
    },
  );
});

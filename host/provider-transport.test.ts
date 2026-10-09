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
let codexClient;
const send = value => process.stdout.write(JSON.stringify(value) + '\\n');
// Records what each turn actually received, so tests can prove that settings
// applied between turns reach the provider.
const record = value => require('node:fs').appendFileSync(require('node:path').join(__dirname, 'calls.log'), JSON.stringify(value) + '\\n');
if (!process.argv.includes('app-server')) record({claudeArgs: process.argv.slice(2)});
readline.createInterface({input: process.stdin}).on('line', line => {
  const request = JSON.parse(line);
  if (request.jsonrpc === '2.0') {
    if (request.id == null) return;
    if (request.method === 'session/prompt') {
      send({jsonrpc: '2.0', method: 'session/update', params: {sessionId: 'fixture_acp', update: {sessionUpdate: 'agent_message_chunk', content: {type: 'text', text: 'Headless ACP completed'}}}});
      setTimeout(() => send({jsonrpc: '2.0', id: request.id, result: {stopReason: 'end_turn'}}), 30);
    } else {
      send({jsonrpc: '2.0', id: request.id, result: request.method === 'session/new' || request.method === 'session/load' || request.method === 'session/resume' ? {sessionId: 'fixture_acp', configOptions: []} : {}});
    }
    return;
  }
  if (request.method === 'initialize') {
    codexClient = request.params.clientInfo.name;
    send({id: request.id, result: {}});
  }
  if (request.method === 'account/read') send({id: request.id, result: {account: {type: 'fixture'}, requiresOpenaiAuth: false}});
  if (request.method === 'model/list') send({id: request.id, result: {data: [{model: 'fixture-model', displayName: 'Fixture model', supportedReasoningEfforts: ['low', 'high']}], nextCursor: null}});
  if (request.method === 'thread/start' || request.method === 'thread/resume') send({id: request.id, result: {thread: {id: 'fixture-thread'}}});
  if (request.method === 'turn/start') {
    record({codexEffort: request.params.effort ?? null, codexClient});
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
  if (request.type === 'user') {
    const replay = (uuid, parentToolUseId = null) => send({type: 'user', isReplay: true, uuid, parent_tool_use_id: parentToolUseId, session_id: 'fixture-claude', message: request.message});
    const complete = () => {
      send({type: 'assistant', session_id: 'fixture-claude', message: {content: [{type: 'text', text: 'Headless Claude completed'}]}});
      send({type: 'result', subtype: 'success', session_id: 'fixture-claude'});
    };
    if (JSON.stringify(request.message).includes('wait-for-current-user-echo')) {
      record({claudeUserUuid: request.uuid});
      replay('11111111-1111-4111-8111-111111111111');
      replay(request.uuid, 'stale-child-tool');
      send({type: 'assistant', session_id: 'fixture-claude', message: {content: [{type: 'text', text: 'Stale buffered Claude answer'}]}});
      const gate = require('node:path').join(__dirname, 'accept-current-claude-user');
      const timer = setInterval(() => {
        if (!require('node:fs').existsSync(gate)) return;
        clearInterval(timer);
        if (process.argv.includes('--replay-user-messages')) replay(request.uuid);
        complete();
      }, 10);
    } else {
      if (process.argv.includes('--replay-user-messages')) replay(request.uuid);
      setTimeout(complete, 30);
    }
  }
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

  it("accepts shared history only after the current Claude user UUID is replayed by the parent", async () => {
    const project = await engine.openProject(directory);
    const { sessionId } = engine.command({
      type: "create", commandId: "echo-create", projectId: project.id,
      harness: "codex", model: "codex:test", runtimeMode: "supervised",
    });
    engine.command({ type: "send", commandId: "echo-source", sessionId, text: "Source requirement" });
    await vi.waitFor(() => expect(store.session(sessionId).status).toBe("idle"), { timeout: 4_000 });
    engine.command({
      type: "switchProvider", commandId: "echo-switch", sessionId,
      expectedRevision: store.session(sessionId).revision,
      harness: "claude", model: "claude:test", modelSettings: {}, runtimeMode: "supervised",
    });
    const beforeSend = store.session(sessionId).revision;
    engine.command({ type: "send", commandId: "echo-target", sessionId, text: "wait-for-current-user-echo" });
    const gate = join(directory, "accept-current-claude-user");
    try {
      await vi.waitFor(() => expect(store.session(sessionId).session.blocks.some((block) =>
        block.text.includes("Stale buffered Claude answer"))).toBe(true), { timeout: 4_000 });
      const waiting = store.session(sessionId).session;
      expect(waiting.providerContext?.delivery?.status).toBe("preparing");
      expect(waiting.pendingSwitch?.fromProviderSessionId).toBe("fixture-thread");
      const waitingEvents = store.events(sessionId, beforeSend).events as Array<{ event: { type: string } }>;
      expect(waitingEvents.filter(({ event }) => event.type === "providerContext.accepted" || event.type === "providerContext.delivered"))
        .toHaveLength(0);
      const calls = readFileSync(join(directory, "calls.log"), "utf8").trim().split("\n").map((line) => JSON.parse(line));
      const request = calls.find((call) => call.claudeUserUuid);
      expect(request.claudeUserUuid).toMatch(/^[\da-f]{8}-[\da-f]{4}-4[\da-f]{3}-[89ab][\da-f]{3}-[\da-f]{12}$/i);
      expect(calls.some((call) => call.claudeArgs?.includes("--replay-user-messages"))).toBe(true);

      writeFileSync(gate, "replay the current parent user");
      await vi.waitFor(() => expect(store.session(sessionId).status).toBe("idle"), { timeout: 4_000 });
      expect(store.session(sessionId).session.providerContext?.delivery).toMatchObject({ status: "accepted", mode: "inline" });
      expect(store.session(sessionId).session.pendingSwitch).toBeUndefined();
      const acceptedEvents = store.events(sessionId, beforeSend).events as Array<{ event: { type: string } }>;
      expect(acceptedEvents.filter(({ event }) => event.type === "providerContext.accepted")).toHaveLength(1);
      expect(acceptedEvents.filter(({ event }) => event.type === "providerContext.delivered")).toHaveLength(1);
    } finally {
      // Release the fixture even when a receipt assertion fails.
      writeFileSync(gate, "release fixture");
      await vi.waitFor(() => expect(store.session(sessionId).status).toBe("idle"), { timeout: 4_000 });
      rmSync(gate, { force: true });
    }
  });

  it.each(["providerContext.delivered", "providerContext.accepted"])(
    "contains a %s storage failure in the real Claude stdout callback", async (failedEvent) => {
      const project = await engine.openProject(directory);
      const key = failedEvent.replaceAll(".", "-");
      const { sessionId } = engine.command({
        type: "create", commandId: `${key}-create`, projectId: project.id,
        harness: "codex", model: "codex:test", runtimeMode: "supervised",
      });
      engine.command({ type: "send", commandId: `${key}-source`, sessionId, text: "Preserve this source requirement" });
      await vi.waitFor(() => expect(store.session(sessionId).status).toBe("idle"), { timeout: 4_000 });
      engine.command({
        type: "switchProvider", commandId: `${key}-switch`, sessionId,
        expectedRevision: store.session(sessionId).revision,
        harness: "claude", model: "claude:test", modelSettings: {}, runtimeMode: "supervised",
      });
      const targetCommandId = `${key}-target`;
      const save = store.save.bind(store);
      let failed = false;
      const write = vi.spyOn(store, "save").mockImplementation((value, event) => {
        const receipt = event as { type?: string; switchId?: string };
        if (!failed && receipt.type === failedEvent && receipt.switchId === targetCommandId) {
          failed = true;
          throw new Error("Injected receipt storage failure");
        }
        return save(value, event);
      });
      try {
        engine.command({ type: "send", commandId: targetCommandId, sessionId, text: "Current request was submitted exactly once" });
        await vi.waitFor(() => expect(store.session(sessionId).status).toBe("interrupted"), { timeout: 4_000 });
        const recovered = store.session(sessionId).session;
        expect(failed).toBe(true);
        expect(recovered.providerContext?.delivery).toMatchObject({ status: "accepted", mode: "inline", requestSubmitted: true });
        expect(recovered.providerSessionId).toBe("fixture-claude");
        expect(recovered.providerContext?.bindings.map((binding) => binding.providerSessionId)).toEqual(["fixture-thread", "fixture-claude"]);
        const requests = recovered.blocks.filter((block) => block.id === targetCommandId);
        expect(requests).toHaveLength(1);
        expect(requests[0].draft).not.toBe(true);
        expect(recovered.pendingSwitch).toBeUndefined();
      } finally {
        write.mockRestore();
      }
    },
  );

  it.each(["cursor", "grok", "fx", "hermes", "antigravity"] as const)(
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
        if (harness === "codex") {
          const mainTurns = calls.filter((call) => call.codexClient === "monocode");
          expect(mainTurns).toHaveLength(1);
          efforts.push(mainTurns[0].codexEffort);
        } else {
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

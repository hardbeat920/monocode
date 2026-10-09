import { describe, expect, it } from "vitest";
import { harnessSupportsAttachments, RUNTIME_MODES } from "../../../../features/sessions/model/session";
import { ATTACHMENT_ONLY_PROMPT } from "../../../../features/sessions/model/attachments";
import * as antigravity from "./antigravityProtocol";
import { AcpSubagents } from "../../core/acpSubagents";

const providers = [
  { id: "antigravity", protocol: antigravity, mode: antigravity.antigravityModeId,
    blocks: antigravity.antigravityPromptBlocks,
    modes: ["default", "auto_edit", "default", "yolo"], plan: "default" },
] as const;

describe.each(providers)("$id ACP protocol", ({ id, protocol, mode, blocks, modes, plan }) => {
  it("maps every runtime mode and overrides full access for planning", () => {
    expect(RUNTIME_MODES.map((runtimeMode) => mode(runtimeMode))).toEqual(modes);
    for (const runtimeMode of RUNTIME_MODES) expect(mode(runtimeMode, true)).toBe(plan);
  });

  it("delivers image-only prompts and leaves attachments enabled", () => {
    const image = {
      id: "image", name: "image.png", kind: "image", mimeType: "image/png",
      size: 4, data: "aGV5",
    } as const;
    expect(harnessSupportsAttachments(id)).toBe(true);
    expect(blocks("", [image])).toEqual([
      { type: "text", text: ATTACHMENT_ONLY_PROMPT },
      { type: "image", mimeType: "image/png", data: "aGV5" },
    ]);
    expect(blocks(" hi ")).toEqual([{ type: "text", text: "hi" }]);
    expect(blocks("  ")).toEqual([]);
  });

  it("extracts session IDs and rejects malformed IDs", () => {
    for (const key of ["sessionId", "session_id", "id"]) {
      expect(protocol.sessionIdFromResult({ [key]: " S1 " })).toBe("S1");
    }
    for (const raw of [null, {}, { sessionId: " " }, { sessionId: 42 }]) {
      expect(protocol.sessionIdFromResult(raw)).toBeUndefined();
    }
  });

  it("resolves config IDs by category without selecting the provider", () => {
    const options = protocol.readConfigOptions([
      null, {}, { id: "provider", category: "model" },
      { id: "model_picker", category: "model", currentValue: "m1" },
      { id: "thinking", category: "thought_level", currentValue: "high" },
    ]);
    expect(options).toHaveLength(3);
    expect(protocol.extractModelConfigId(options)).toBe("model_picker");
    expect(protocol.extractModelConfigId([])).toBe("model");
    expect(protocol.resolveSettingConfigId(options, "effort")).toBe("thinking");
    expect(protocol.resolveSettingConfigId(options, "reasoning")).toBe("thinking");
    expect(protocol.resolveSettingConfigId(options, "THINKING")).toBe("thinking");
    expect(protocol.resolveSettingConfigId(options, "missing")).toBeUndefined();
  });

  it("discovers live models and thinking levels, including grouped choices", () => {
    const models = protocol.modelsFromSessionNew({ configOptions: [
      { id: "model", category: "model", options: [
        { group: "provider", options: [
          { value: "m1", name: "Model One" }, { value: "m2", name: "Model Two" },
        ] },
      ] },
      { id: "thinking", category: "thought_level", currentValue: "high", options: [
        { value: "low", name: "Low" }, { value: "high", name: "High" },
        { value: "max", name: "Max" },
      ] },
    ] });
    expect(models.map((model) => model.id)).toEqual([`${id}:m1`, `${id}:m2`]);
    expect(models[0]).toMatchObject({ harness: id, name: "Model One", nativeId: "m1",
      settings: [{ id: "effort", value: "high", options: [
        { value: "low", label: "Low" }, { value: "high", label: "High" },
        { value: "max", label: "Max" },
      ] }],
    });
    expect(protocol.modelsFromSessionNew(null)).toEqual([]);
  });

  it("falls back to top-level ACP models without inventing reasoning controls", () => {
    expect(protocol.modelsFromSessionNew({ models: { availableModels: [
      { modelId: "gemini-pro-agent", name: "Gemini 3.1 Pro (High)" },
    ] } })).toEqual([{
      id: `${id}:gemini-pro-agent`, harness: id, nativeId: "gemini-pro-agent",
      name: "Gemini 3.1 Pro (High)",
    }]);
  });

  it("uses opaque permission IDs by semantic kind and never fabricates an ID", () => {
    const request = protocol.permissionRequestFromAcp({
      toolCall: { toolCallId: "t1", title: "Write file", kind: "edit" },
      options: [
        { optionId: "yes-7", kind: "allow_once" },
        { optionId: "no-9", kind: "reject_once" },
      ],
    });
    expect(request).toMatchObject({ callId: "t1", kind: "edit" });
    expect(protocol.permissionOptionId("allow", request.optionIds, request.optionKinds)).toBe("yes-7");
    expect(protocol.permissionOptionId("deny", request.optionIds, request.optionKinds)).toBe("no-9");
    expect(protocol.permissionOptionId("deny", ["allow_once"])).toBeNull();
    expect(protocol.permissionOptionId("allow", [])).toBeNull();
    expect(protocol.autoPermissionOption("supervised", "edit", request.optionIds, request.optionKinds)).toBeNull();
    expect(protocol.autoPermissionOption("auto", "execute", request.optionIds, request.optionKinds)).toBeNull();
    expect(protocol.autoPermissionOption("auto-accept-edits", "execute", request.optionIds, request.optionKinds)).toBeNull();
    expect(protocol.autoPermissionOption("auto-accept-edits", "edit", request.optionIds, request.optionKinds)).toBe("yes-7");
    expect(protocol.autoPermissionOption("full-access", "execute", request.optionIds, request.optionKinds)).toBe("yes-7");
  });

  it("maps text, reasoning, tools and plans without fx-specific result decoding", () => {
    const parse = (update: unknown) => protocol.eventsFromAcpUpdate({ update });
    expect(parse({ sessionUpdate: "agent_message_chunk", content: { type: "text", text: "hi" } }))
      .toEqual([{ type: "message.delta", text: "hi" }]);
    expect(parse({ sessionUpdate: "agent_thought_chunk", content: { text: "think" } }))
      .toEqual([{ type: "reasoning.delta", text: "think" }]);
    expect(parse({ sessionUpdate: "tool_call", toolCallId: "t", title: "Read file", kind: "read", status: "completed" }))
      .toMatchObject([{ type: "tool.updated", callId: "t", kind: "read", status: "completed" }]);
    expect(parse({ sessionUpdate: "plan", entries: [{ content: "Check code", status: "pending" }] }))
      .toMatchObject([{ type: "tasks.updated", items: [{ text: "Check code" }] }]);
    expect(parse({ sessionUpdate: "available_commands_update", availableCommands: [] })).toEqual([]);
  });
});

describe("Antigravity subagents", () => {
  it("gives each child conversation its own row until the turn ends", () => {
    const session = "b9f875bb-f007-41cc-b084-54110d90e953";
    const spawns = new antigravity.AntigravitySubagents();
    const router = new AcpSubagents();
    const feed = (update: Record<string, unknown>) => {
      const params = { sessionId: session, update };
      const child = spawns.child(params, session);
      return [
        ...(child?.opened ? router.route({}, [child.opened]) : []),
        ...spawns.label(router.route(params, antigravity.eventsFromAcpUpdate(params), child?.parent)),
      ];
    };
    const a = "bea70cf1-72ab-404f-9ab3-8ae15730f544";
    const b = "d0a25b38-114b-4cb4-a214-a35fac299080";

    // Recorded from agy_acp_server 1.3.0.
    expect(feed({ sessionUpdate: "tool_call", toolCallId: `${session}:2`, title: "Running start_subagent", kind: "other", status: "in_progress", rawInput: {} }))
      .toMatchObject([{ type: "tool.updated", callId: `${session}:2`, kind: "other", title: "Launch subagents" }]);
    expect(feed({ sessionUpdate: "tool_call", toolCallId: `${a}:1`, title: "ls -la", kind: "execute", status: "in_progress", rawInput: { command_line: "ls -la" } }))
      .toMatchObject([
        { type: "tool.updated", callId: `subagent:${a}`, kind: "agent", title: "Subagent", status: "in_progress" },
        { type: "agent.step", callId: `subagent:${a}`, stepId: `tool:${a}:1`, kind: "tool", status: "in_progress" },
      ]);
    expect(feed({ sessionUpdate: "tool_call", toolCallId: `${b}:1`, title: "Running list_directory", kind: "search", status: "in_progress" }))
      .toMatchObject([
        { type: "tool.updated", callId: `subagent:${b}`, title: "Subagent" },
        { type: "agent.step", callId: `subagent:${b}`, stepId: `tool:${b}:1` },
      ]);
    expect(feed({ sessionUpdate: "tool_call_update", toolCallId: `${a}:1`, status: "completed" }))
      .toEqual([expect.objectContaining({ type: "agent.step", callId: `subagent:${a}`, status: "completed" })]);
    // Bare ids carry no conversation, and the parent's own calls stay top-level.
    expect(feed({ sessionUpdate: "tool_call", toolCallId: "call_713448", title: "Running view_file", kind: "read" }))
      .toMatchObject([{ type: "tool.updated", callId: "call_713448" }]);
    expect(feed({ sessionUpdate: "tool_call", toolCallId: `${session}:3`, title: "pwd", kind: "execute" }))
      .toMatchObject([{ type: "tool.updated", callId: `${session}:3` }]);

    // A role read after its row opened renames it, through its latest step too:
    // a run that already has steps keeps the name they carry.
    expect(spawns.learn([
      { conversationId: a, role: "Directory Lister", typeName: "research" },
      { conversationId: a, role: "Ignored duplicate" },
    ])).toMatchObject([
      { type: "tool.updated", callId: `subagent:${a}`, title: "Directory Lister" },
      { type: "agent.step", callId: `subagent:${a}`, stepId: `tool:${a}:1`, status: "completed", agentName: "Directory Lister", agentType: "research" },
    ]);
    expect(feed({ sessionUpdate: "tool_call", toolCallId: `${a}:2`, title: "pwd", kind: "execute" }))
      .toMatchObject([{ type: "agent.step", callId: `subagent:${a}`, agentName: "Directory Lister", agentType: "research" }]);

    expect(spawns.settle()).toEqual([
      { type: "tool.updated", callId: `subagent:${a}`, status: "completed" },
      { type: "tool.updated", callId: `subagent:${b}`, status: "completed" },
    ]);
    expect(spawns.settle()).toEqual([]);
  });

  it("relabels only the delegation tool, not a command naming it", () => {
    const title = (update: Record<string, unknown>) =>
      antigravity.eventsFromAcpUpdate({ update: { sessionUpdate: "tool_call", toolCallId: "call_1", ...update } })[0];
    expect(title({ title: "Running invoke_subagent", kind: "other" })).toMatchObject({ title: "Launch subagents" });
    const search = title({ title: "rg invoke_subagent src/", kind: "execute", rawInput: { command_line: "rg invoke_subagent src/" } });
    expect(search).toMatchObject({ kind: "execute" });
    expect(search).not.toMatchObject({ title: "Launch subagents" });
  });

  it("attributes only lowercase conversation ids, as the store does", () => {
    const spawns = new antigravity.AntigravitySubagents();
    const session = "639c8fd4-acaa-451e-acbf-db8dffbce7cb";
    expect(spawns.child({ update: { toolCallId: "C8D11A3C-960B-4B7E-B7BF-1684938D2F31:1" } }, session)).toBeUndefined();
    expect(spawns.child({ update: { toolCallId: "c8d11a3c-960b-4b7e-b7bf-1684938d2f31:1" } }, session)).toBeDefined();
  });

  it("spaces store reads and gives up on a child the store never names", () => {
    const session = "639c8fd4-acaa-451e-acbf-db8dffbce7cb";
    const a = "bea70cf1-72ab-404f-9ab3-8ae15730f544";
    const b = "d0a25b38-114b-4cb4-a214-a35fac299080";
    const spawns = new antigravity.AntigravitySubagents();
    expect(spawns.nextLookup(0)).toBeUndefined();

    spawns.child({ update: { toolCallId: `${a}:1` } }, session);
    expect(spawns.nextLookup(0)).toBe(0);
    spawns.lookedUp(0);
    expect(spawns.nextLookup(400)).toBe(600);
    expect(spawns.nextLookup(1_000)).toBe(0);
    for (const now of [1_000, 2_000, 3_000, 4_000]) spawns.lookedUp(now);
    // Five reads spent: an update from `a` no longer costs one.
    expect(spawns.nextLookup(60_000)).toBeUndefined();

    // A later sibling brings its own reads, and still waits out the gap.
    spawns.child({ update: { toolCallId: `${b}:1` } }, session);
    expect(spawns.nextLookup(4_500)).toBe(500);
    spawns.learn([{ conversationId: b, role: "Tester" }]);
    expect(spawns.nextLookup(9_000)).toBeUndefined();

    // The next turn's children start afresh.
    spawns.settle();
    spawns.child({ update: { toolCallId: `${a}:2` } }, session);
    expect(spawns.nextLookup(9_000)).toBe(0);
  });

  it("opens a child already named under its role", () => {
    const session = "639c8fd4-acaa-451e-acbf-db8dffbce7cb";
    const child = "c8d11a3c-960b-4b7e-b7bf-1684938d2f31";
    const spawns = new antigravity.AntigravitySubagents();
    expect(spawns.learn([{ conversationId: child, role: "Frontend App Researcher" }])).toEqual([]);
    const opened = spawns.child({ update: { toolCallId: `${child}:1` } }, session);
    expect(opened).toMatchObject({
      opened: { callId: `subagent:${child}`, kind: "agent", title: "Frontend App Researcher" },
    });
    expect(opened).not.toHaveProperty("unnamed");
  });
});

import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  agentCommands,
  agentCommandPrompt,
  filterSlashItems,
  runChatCommand,
} from "./agentCommands";
import { resetHarnessModelOverlays, setHarnessModels } from "./models";
import { newSession, type Session } from "./session";
import type { Skill } from "./skills";

const skill: Skill = {
  kind: "file",
  name: "status",
  invocation: "status",
  description: "Project status",
  path: "/repo/.agents/skills/status/SKILL.md",
  scope: "project",
  source: "agents",
};
const handlers = () => ({
  onModelChange: vi.fn(),
  onModelSettingsChange: vi.fn(),
  onRuntimeModeChange: vi.fn(),
  onCompactContext: vi.fn(() => true),
  onStop: vi.fn(),
  onOpenDiff: vi.fn(),
});
function session(harness: "codex" | "claude" = "codex"): Session {
  return {
    ...newSession(harness, "/repo", `${harness}:current`),
    id: "chat-1",
    providerSessionId: "existing-provider-history",
  };
}
beforeEach(() => {
  resetHarnessModelOverlays();
  for (const harness of ["codex", "claude"] as const)
    setHarnessModels(harness, [
      {
        id: `${harness}:current`,
        harness,
        name: "Current model",
        nativeId: "current",
        settings: [
          {
            id: "effort",
            label: "Effort",
            kind: "select",
            value: "low",
            options: [
              { value: "low", label: "Low" },
              { value: "high", label: "High" },
            ],
          },
          {
            id: "fast",
            label: "Fast",
            kind: "toggle",
            value: "false",
            options: [],
          },
        ],
      },
      { id: `${harness}:next`, harness, name: "Next model", nativeId: "next" },
    ]);
});

describe("agent command routing", () => {
  it.each(["codex", "claude"] as const)(
    "recognizes %s commands and legacy aliases without changing skill semantics",
    (harness) => {
      expect(agentCommandPrompt("/status", harness, "all", [])).toBe("/status");
      expect(
        agentCommandPrompt("/agent:model next", harness, "skills", [skill]),
      ).toBe("/model next");
      expect(agentCommandPrompt("/cli:status", harness, "all", [skill])).toBe(
        "/status",
      );
      expect(agentCommandPrompt("/status", harness, "all", [skill])).toBeNull();
      expect(agentCommandPrompt("/status", harness, "skills", [])).toBeNull();
      expect(
        agentCommandPrompt("/future-command", harness, "commands", []),
      ).toBe("/future-command");
      expect(agentCommandPrompt("/compact", harness, "all", [])).toBeNull();
      expect(agentCommandPrompt("/plan fix it", harness, "all", [])).toBeNull();
      expect(
        agentCommandPrompt("Discuss /status", harness, "commands", []),
      ).toBeNull();
      expect(
        agentCommandPrompt("> /status", harness, "commands", []),
      ).toBeNull();
      expect(agentCommandPrompt("/status", "omp", "commands", [])).toBeNull();
    },
  );
  it("labels unsupported commands instead of advertising terminal execution", () => {
    const commands = agentCommands("claude");
    expect(commands.find((entry) => entry.name === "theme")).toMatchObject({
      origin: "Not supported in chat",
    });
    expect(commands.find((entry) => entry.name === "status")).toMatchObject({
      origin: "Chat",
      invocation: "agent:status",
    });
    expect(filterSlashItems([skill, ...commands], "skills")).toEqual([skill]);
    expect(filterSlashItems([skill, ...commands], "commands")).toEqual(
      commands,
    );
    expect(agentCommands("cursor")).toEqual([]);
  });
});

describe("commands in the existing conversation", () => {
  it.each(["codex", "claude"] as const)(
    "reads %s status without mutating the session or invoking an action",
    (harness) => {
      const current = session(harness);
      const snapshot = structuredClone(current);
      const actions = handlers();
      expect(runChatCommand(current, "/status", actions)).toMatchObject({
        accepted: true,
        panel: { kind: "status" },
      });
      expect(current).toEqual(snapshot);
      Object.values(actions).forEach((action) =>
        expect(action).not.toHaveBeenCalled(),
      );
    },
  );
  it("changes the existing session's model and permissions through its normal callbacks", () => {
    const actions = handlers();
    expect(runChatCommand(session(), "/model next", actions).accepted).toBe(
      true,
    );
    expect(actions.onModelChange).toHaveBeenCalledWith(
      "chat-1",
      "codex",
      "codex:next",
    );
    expect(
      runChatCommand(session(), "/permissions supervised", actions).accepted,
    ).toBe(true);
    expect(actions.onRuntimeModeChange).toHaveBeenCalledWith(
      "chat-1",
      "supervised",
    );
    expect(runChatCommand(session(), "/model missing", actions).accepted).toBe(
      false,
    );
    expect(actions.onModelChange).toHaveBeenCalledTimes(1);
  });
  it("uses only settings advertised by the current model, preserving other settings", () => {
    const current = {
      ...session(),
      modelSettings: { effort: "low", other: "keep" },
    };
    const actions = handlers();
    expect(runChatCommand(current, "/fast on", actions).accepted).toBe(true);
    expect(actions.onModelSettingsChange).toHaveBeenCalledWith("chat-1", {
      effort: "low",
      other: "keep",
      fast: "true",
    });
    expect(runChatCommand(current, "/reasoning high", actions).accepted).toBe(
      true,
    );
    expect(actions.onModelSettingsChange).toHaveBeenLastCalledWith("chat-1", {
      effort: "high",
      other: "keep",
    });
    expect(
      runChatCommand(current, "/reasoning imaginary", actions).accepted,
    ).toBe(false);
    expect(
      runChatCommand({ ...current, model: "codex:next" }, "/fast on", actions)
        .accepted,
    ).toBe(false);
    expect(actions.onModelSettingsChange).toHaveBeenCalledTimes(2);
  });
  it("compacts the same conversation and honors rejected compaction", () => {
    const actions = handlers();
    expect(runChatCommand(session(), "/compact", actions).accepted).toBe(true);
    expect(actions.onCompactContext).toHaveBeenCalledWith("chat-1");
    actions.onCompactContext.mockReturnValue(false);
    expect(runChatCommand(session(), "/compact", actions).accepted).toBe(false);
  });
  it.each([
    { busy: true },
    { queuedMessages: [{ id: "queued", text: "follow up", attachments: [] }] },
  ])("protects settings and history while work is pending: %j", (patch) => {
    const actions = handlers();
    const current = { ...session(), ...patch };
    for (const command of [
      "/model next",
      "/permissions full-access",
      "/compact",
      "/fast on",
    ])
      expect(runChatCommand(current, command, actions).accepted).toBe(false);
    expect(runChatCommand(current, "/status", actions).accepted).toBe(true);
    Object.values(actions).forEach((action) =>
      expect(action).not.toHaveBeenCalled(),
    );
    expect(runChatCommand(current, "/stop", actions).accepted).toBe(true);
    expect(actions.onStop).toHaveBeenCalledWith("chat-1");
  });
  it("rejects unsupported commands and invalid input without invoking any action", () => {
    const actions = handlers();
    for (const text of [
      "/theme",
      "/future-command",
      "/logout",
      "/clear",
      "/status\n/logout",
      "/status\x1b[A",
      "/status\t",
      "/compact extra",
      "status",
    ])
      expect(runChatCommand(session(), text, actions).accepted).toBe(false);
    Object.values(actions).forEach((action) =>
      expect(action).not.toHaveBeenCalled(),
    );
  });
});

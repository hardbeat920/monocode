import { describe, expect, it } from "vitest";
import {
  commandsFromCopilotUpdate,
  copilotError,
  copilotPermissionOption,
  copilotSpawnArgs,
  modelsFromCopilotSession,
} from "./copilotProtocol";

describe("Copilot ACP metadata", () => {
  // Copilot CLI 1.0.94 advertises modelId "auto" and accepts session/set_model.
  it("uses the native catalog with the current model first and ignores invalid entries", () => {
    expect(
      modelsFromCopilotSession({
        models: {
          currentModelId: "auto",
          availableModels: [
            { modelId: "gpt-4.1", name: "GPT 4.1" },
            { modelId: "auto", name: "Auto" },
            { modelId: "auto", name: "Duplicate" },
            { modelId: 123 },
            null,
          ],
        },
      }),
    ).toEqual([
      {
        id: "copilot:auto",
        harness: "copilot",
        nativeId: "auto",
        name: "Auto",
      },
      {
        id: "copilot:gpt-4.1",
        harness: "copilot",
        nativeId: "gpt-4.1",
        name: "GPT 4.1",
      },
    ]);
    expect(modelsFromCopilotSession({})).toEqual([]);
  });

  it("starts ACP without spawn-time reasoning settings or allow-all", () => {
    expect(copilotSpawnArgs()).toEqual([
      "--acp",
      "--stdio",
      "--no-auto-update",
    ]);
  });

  it("never guesses an unadvertised permission option", () => {
    expect(
      copilotPermissionOption(
        { options: [{ optionId: "opaque", kind: "allow_once" }] },
        true,
      ),
    ).toBe("opaque");
    expect(
      copilotPermissionOption(
        { options: [{ optionId: "opaque", kind: "allow_once" }] },
        false,
      ),
    ).toBeUndefined();
    expect(copilotPermissionOption({}, true)).toBeUndefined();
  });

  it("discovers only advertised commands and escapes app-owned names", () => {
    expect(
      commandsFromCopilotUpdate({
        update: {
          sessionUpdate: "available_commands_update",
          availableCommands: [
            { name: "plan", description: "Plan", input: { hint: "prompt" } },
            { name: 123 },
          ],
        },
      }),
    ).toEqual([
      {
        name: "plan",
        invocation: "copilot:plan",
        source: "copilot",
        description: "Plan",
        inputHint: "prompt",
      },
    ]);
    expect(
      commandsFromCopilotUpdate({
        update: { sessionUpdate: "agent_message_chunk" },
      }),
    ).toBeUndefined();
  });

  it("adds actionable login help only to authentication failures", () => {
    expect(
      copilotError(new Error("Authentication required")).message,
    ).toContain("copilot login");
    expect(copilotError(new Error("Session not found")).message).toBe(
      "Session not found",
    );
  });
});

// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
import {
  CLAUDE_EXTRAS_DEFAULT,
  HARNESS_RUNTIME_DEFAULT,
  loadClaudeExtras,
  loadHarnessRuntime,
  saveClaudeExtras,
  saveHarnessRuntime,
} from "./settings";

beforeEach(() => {
  localStorage.clear();
});

describe("loadHarnessRuntime / saveHarnessRuntime", () => {
  it("defaults to empty for a harness with no saved runtime", () => {
    expect(loadHarnessRuntime("claude")).toEqual(HARNESS_RUNTIME_DEFAULT);
  });

  it("round-trips binary path, launch args, and env vars for one harness", () => {
    const next = {
      binaryPath: "/custom/claude",
      launchArgs: "--chrome --foo",
      env: [{ key: "FOO", value: "bar" }],
    };
    saveHarnessRuntime("claude", next);
    expect(loadHarnessRuntime("claude")).toEqual(next);
  });

  it("keeps each harness's runtime settings independent", () => {
    saveHarnessRuntime("claude", {
      binaryPath: "/custom/claude",
      launchArgs: "",
      env: [],
    });
    saveHarnessRuntime("codex", {
      binaryPath: "/custom/codex",
      launchArgs: "",
      env: [],
    });
    expect(loadHarnessRuntime("claude").binaryPath).toBe("/custom/claude");
    expect(loadHarnessRuntime("codex").binaryPath).toBe("/custom/codex");
    expect(loadHarnessRuntime("cursor")).toEqual(HARNESS_RUNTIME_DEFAULT);
  });

  it("ignores malformed stored JSON and falls back to the default", () => {
    localStorage.setItem("monocode.harnessRuntime", "not json");
    expect(loadHarnessRuntime("claude")).toEqual(HARNESS_RUNTIME_DEFAULT);
  });
});

describe("loadClaudeExtras / saveClaudeExtras", () => {
  it("defaults to empty when nothing is saved", () => {
    expect(loadClaudeExtras()).toEqual(CLAUDE_EXTRAS_DEFAULT);
  });

  it("round-trips configDir", () => {
    const next = { configDir: "/custom/claude-home" };
    saveClaudeExtras(next);
    expect(loadClaudeExtras()).toEqual(next);
  });
});

// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
import {
  HARNESS_RUNTIME_DEFAULT,
  loadClaudeConfigDir,
  loadHarnessRuntime,
  saveClaudeConfigDir,
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

describe("loadClaudeConfigDir / saveClaudeConfigDir", () => {
  it("defaults to empty when nothing is saved", () => {
    expect(loadClaudeConfigDir()).toBe("");
  });

  it("round-trips the config dir", () => {
    saveClaudeConfigDir("/custom/claude-home");
    expect(loadClaudeConfigDir()).toBe("/custom/claude-home");
  });
});

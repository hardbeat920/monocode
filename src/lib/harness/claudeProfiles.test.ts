import { describe, expect, it } from "vitest";
import {
  claudeProfileOptionsFor,
  resolveClaudeProfileEnv,
} from "./claudeProfiles";

describe("claudeProfileOptionsFor", () => {
  it("allows all three profiles everywhere", () => {
    expect(
      claudeProfileOptionsFor("/Users/cartrabbit/Documents/personal/poynt").map(
        (option) => option.value,
      ),
    ).toEqual(["personal", "nishanth", "benitto"]);
    expect(
      claudeProfileOptionsFor(
        "/Users/cartrabbit/Documents/yuko/yuko-backend",
      ).map((option) => option.value),
    ).toEqual(["personal", "nishanth", "benitto"]);
  });
});

describe("resolveClaudeProfileEnv", () => {
  const home = "/Users/cartrabbit";
  const cwd = "/Users/cartrabbit/Documents/personal/poynt";

  it("Personal has no CLAUDE_CONFIG_DIR override — the plain default account", () => {
    const result = resolveClaudeProfileEnv("personal", cwd, home);
    expect(result.profileId).toBe("personal");
    expect(result.env).toEqual({});
    expect(result.warning).toBeUndefined();
  });

  it("Nishanth points CLAUDE_CONFIG_DIR at claudo's dir", () => {
    const result = resolveClaudeProfileEnv("nishanth", cwd, home);
    expect(result.profileId).toBe("nishanth");
    expect(result.env.CLAUDE_CONFIG_DIR).toBe("/Users/cartrabbit/.claude-personal");
  });

  it("Benitto points CLAUDE_CONFIG_DIR at claudz's dir", () => {
    const result = resolveClaudeProfileEnv("benitto", cwd, home);
    expect(result.profileId).toBe("benitto");
    expect(result.env.CLAUDE_CONFIG_DIR).toBe("/Users/cartrabbit/.claude-office2");
  });

  it("defaults to Personal (no override) when no profile was requested", () => {
    const result = resolveClaudeProfileEnv(undefined, cwd, home);
    expect(result.profileId).toBe("personal");
    expect(result.env).toEqual({});
    expect(result.warning).toBeUndefined();
  });
});

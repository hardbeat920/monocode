import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { newSession, newSessionForProject } from "./session";
import { setProjectProviderHidden } from "./projectProviders";

describe("newSessionForProject", () => {
  beforeEach(() => {
    const storage = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => {
        storage.set(key, value);
      },
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("keeps a seed provider the project allows", () => {
    const seed = newSession("claude", "/repo/a", "claude:opus-5");
    const session = newSessionForProject(seed, "/repo/a");
    expect(session.harness).toBe("claude");
    expect(session.model).toBe("claude:opus-5");
  });

  it("falls back to an enabled provider when the seed one is disabled", () => {
    setProjectProviderHidden("/repo/a", "claude", true);
    const seed = newSession("claude", "/repo/a", "claude:opus-5");
    const session = newSessionForProject(seed, "/repo/a");
    expect(session.harness).toBe("codex");
    expect(session.cwd).toBe("/repo/a");
  });

  it("does not carry a seed model across providers", () => {
    setProjectProviderHidden("/repo/a", "claude", true);
    const seed = newSession("claude", "/repo/a", "claude:opus-5");
    const session = newSessionForProject(seed, "/repo/a");
    expect(session.model).not.toBe("claude:opus-5");
  });
});

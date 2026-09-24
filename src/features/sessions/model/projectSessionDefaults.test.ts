import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  newSession,
  newSessionForProject,
  retargetSessionToProject,
} from "./session";
import {
  setProjectDefaultModel,
  setProjectDefaultProvider,
  setProjectProviderHidden,
} from "./projectProviders";

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

  it("uses the project default provider even when the seed one is allowed", () => {
    setProjectDefaultProvider("/repo/a", "cursor", "cursor:composer-2.5");
    const seed = newSession("claude", "/repo/a", "claude:opus-5");
    const session = newSessionForProject(seed, "/repo/a");
    expect(session.harness).toBe("cursor");
    expect(session.model).toBe("cursor:composer-2.5");
  });

  it("applies a project model override to the carried provider", () => {
    setProjectDefaultModel("/repo/a", "claude", "claude:haiku-4.5");
    const seed = newSession("claude", "/repo/a", "claude:opus-5");
    const session = newSessionForProject(seed, "/repo/a");
    expect(session.harness).toBe("claude");
    expect(session.model).toBe("claude:haiku-4.5");
  });
});

describe("retargetSessionToProject", () => {
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

  it("adopts the project defaults while keeping the session identity", () => {
    setProjectDefaultProvider("/repo/a", "cursor", "cursor:composer-2.5");
    const blank = newSession("claude", "~", "claude:opus-5");
    const retargeted = retargetSessionToProject(blank, "/repo/a");
    expect(retargeted.id).toBe(blank.id);
    expect(retargeted.cwd).toBe("/repo/a");
    expect(retargeted.harness).toBe("cursor");
    expect(retargeted.model).toBe("cursor:composer-2.5");
  });

  it("keeps the session's provider when the project has no defaults", () => {
    const blank = newSession("claude", "~", "claude:opus-5");
    const retargeted = retargetSessionToProject(blank, "/repo/a");
    expect(retargeted.harness).toBe("claude");
    expect(retargeted.model).toBe("claude:opus-5");
  });
});

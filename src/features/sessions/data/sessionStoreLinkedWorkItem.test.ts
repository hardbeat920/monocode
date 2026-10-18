import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  sanitizeLinkedWorkItem,
  setSessionLinkedWorkItem,
} from "./sessionStore";
import { setGithubHost } from "../../inbox/model/githubHost";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => {
  vi.mocked(invoke).mockReset().mockResolvedValue(undefined);
});

describe("setSessionLinkedWorkItem", () => {
  it("persists a canonical GitHub work item through the metadata command", async () => {
    await setSessionLinkedWorkItem("session-1", {
      kind: "pr",
      repo: "openai/codex",
      number: 42,
      url: "https://example.com/untrusted",
    });

    expect(invoke).toHaveBeenCalledWith("session_set_linked_work_item", {
      sessionId: "session-1",
      linkedWorkItem: {
        kind: "pr",
        repo: "openai/codex",
        number: 42,
        url: "https://github.com/openai/codex/pull/42",
      },
    });
  });

  it("uses null to remove a persisted link", async () => {
    await setSessionLinkedWorkItem("session-1", undefined);

    expect(invoke).toHaveBeenCalledWith("session_set_linked_work_item", {
      sessionId: "session-1",
      linkedWorkItem: null,
    });
  });
});

describe("sanitizeLinkedWorkItem", () => {
  it("keeps the server a link was made on after the GitHub host changes", () => {
    setGithubHost("github.example.com");
    try {
      expect(
        sanitizeLinkedWorkItem({
          kind: "pr",
          repo: "acme/web",
          number: 7,
          url: "https://github.com/acme/web/pull/7",
        })?.url,
      ).toBe("https://github.com/acme/web/pull/7");
      expect(
        sanitizeLinkedWorkItem({ kind: "issue", repo: "acme/web", number: 3 })
          ?.url,
      ).toBe("https://github.example.com/acme/web/issues/3");
      expect(
        sanitizeLinkedWorkItem({
          kind: "pr",
          repo: "acme/web",
          number: 7,
          url: "https://evil.example/other/repo/pull/7",
        })?.url,
      ).toBe("https://github.example.com/acme/web/pull/7");
    } finally {
      setGithubHost(null);
    }
  });
});

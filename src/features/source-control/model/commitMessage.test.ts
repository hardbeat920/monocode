import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../platform/tauri/fs", () => ({
  gitCommitMessage: vi.fn(),
}));

import { gitCommitMessage } from "../../../platform/tauri/fs";
import {
  clearCommitMessageCache,
  loadCommitMessage,
  MAX_CACHED_COMMIT_MESSAGES,
  peekCommitMessage,
  splitCommitMessage,
} from "./commitMessage";

const mockMessage = vi.mocked(gitCommitMessage);

beforeEach(() => {
  mockMessage.mockReset();
  // The cache is a module singleton that outlives the test, so a test that
  // reused an earlier test's repo path would be served its cached message and
  // assert nothing about Git.
  clearCommitMessageCache();
});

describe("splitCommitMessage", () => {
  it("splits the subject from its description", () => {
    expect(splitCommitMessage("Subject line\n\nBody text")).toEqual({
      raw: "Subject line\n\nBody text",
      subject: "Subject line",
      description: "Body text",
    });
  });

  it("keeps every line of a multi-paragraph body", () => {
    expect(splitCommitMessage("Subject\n\n- one\n- two\n")).toEqual({
      raw: "Subject\n\n- one\n- two\n",
      subject: "Subject",
      description: "- one\n- two",
    });
  });

  it("returns an empty description for a subject-only commit", () => {
    expect(splitCommitMessage("Just a subject")).toEqual({
      raw: "Just a subject",
      subject: "Just a subject",
      description: "",
    });
  });

  it("skips blank lines before the subject and normalizes CRLF", () => {
    expect(splitCommitMessage("\n\nSubject\r\n\r\nBody")).toEqual({
      raw: "\n\nSubject\r\n\r\nBody",
      subject: "Subject",
      description: "Body",
    });
  });
});

describe("loadCommitMessage", () => {
  it("caches a successful lookup for later peeks", async () => {
    mockMessage.mockResolvedValue("Subject\n\nBody");
    expect(peekCommitMessage("/repo-cache", "sha")).toBeUndefined();

    const first = await loadCommitMessage("/repo-cache", "sha");
    expect(first).toEqual({
      raw: "Subject\n\nBody",
      subject: "Subject",
      description: "Body",
    });
    expect(peekCommitMessage("/repo-cache", "sha")).toEqual(first);

    expect(await loadCommitMessage("/repo-cache", "sha")).toEqual(first);
    expect(mockMessage).toHaveBeenCalledTimes(1);
  });

  it("shares one request between concurrent callers", async () => {
    let resolve!: (text: string) => void;
    mockMessage.mockReturnValue(
      new Promise<string>((r) => {
        resolve = r;
      }),
    );

    const first = loadCommitMessage("/repo-concurrent", "sha");
    const second = loadCommitMessage("/repo-concurrent", "sha");
    expect(mockMessage).toHaveBeenCalledTimes(1);

    resolve("Subject\n\nBody");
    expect(await first).toEqual(await second);
  });

  it("returns null on failure and retries on the next call", async () => {
    mockMessage.mockRejectedValueOnce(new Error("git exploded"));
    expect(await loadCommitMessage("/repo-retry", "sha")).toBeNull();
    expect(peekCommitMessage("/repo-retry", "sha")).toBeUndefined();

    mockMessage.mockResolvedValueOnce("Subject");
    expect(await loadCommitMessage("/repo-retry", "sha")).toEqual({
      raw: "Subject",
      subject: "Subject",
      description: "",
    });
    expect(mockMessage).toHaveBeenCalledTimes(2);
  });

  it("drops the oldest entries once the cache is full", async () => {
    mockMessage.mockResolvedValue("Subject");

    // Two commits past the cap, so the very first one is evicted. Eviction is
    // what stops a long session across many projects growing without bound.
    const first = "sha-0";
    await loadCommitMessage("/repo-cap", first);
    for (let i = 1; i <= MAX_CACHED_COMMIT_MESSAGES; i++) {
      await loadCommitMessage("/repo-cap", `sha-${i}`);
    }

    expect(peekCommitMessage("/repo-cap", first)).toBeUndefined();
    expect(
      peekCommitMessage("/repo-cap", `sha-${MAX_CACHED_COMMIT_MESSAGES}`),
    ).toBeDefined();
    const callsBefore = mockMessage.mock.calls.length;
    await loadCommitMessage("/repo-cap", first);
    expect(mockMessage).toHaveBeenCalledTimes(callsBefore + 1);
  });

  it("does not evict on a failed lookup", async () => {
    mockMessage.mockResolvedValue("Keep me");
    await loadCommitMessage("/repo-cap-fail", "keep-me");
    const callsBefore = mockMessage.mock.calls.length;

    mockMessage.mockRejectedValue(new Error("git exploded"));
    expect(await loadCommitMessage("/repo-cap-fail", "boom")).toBeNull();
    // The failure is not cached, so it costs one call and no cache slot.
    expect(mockMessage).toHaveBeenCalledTimes(callsBefore + 1);

    // The good entry is still there and still cached: no extra Git call.
    expect(peekCommitMessage("/repo-cap-fail", "keep-me")?.subject).toBe(
      "Keep me",
    );
    await loadCommitMessage("/repo-cap-fail", "keep-me");
    expect(mockMessage).toHaveBeenCalledTimes(callsBefore + 1);
  });
});

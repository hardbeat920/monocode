import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../platform/tauri/fs", () => ({
  gitCommitMessage: vi.fn(),
}));

import { gitCommitMessage } from "../../../platform/tauri/fs";
import {
  loadCommitMessage,
  peekCommitMessage,
  splitCommitMessage,
} from "./commitMessage";

const mockMessage = vi.mocked(gitCommitMessage);

beforeEach(() => {
  mockMessage.mockReset();
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
});

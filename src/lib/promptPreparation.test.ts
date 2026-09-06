import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  applyFileMentionsToTurn: vi.fn(),
  applyNotesToTurn: vi.fn(),
  applySkillsToTurn: vi.fn(),
  events: [] as string[],
  warmNativeSkills: vi.fn(),
}));

vi.mock("./fileMentions", () => ({
  applyFileMentionsToTurn: mocks.applyFileMentionsToTurn,
}));

vi.mock("./notes", () => ({
  applyNotesToTurn: mocks.applyNotesToTurn,
}));

vi.mock("./skills", () => ({
  applySkillsToTurn: mocks.applySkillsToTurn,
  warmNativeSkills: mocks.warmNativeSkills,
  isNativeCommandPrompt: (text: string, harness: string) =>
    harness === "omp" && text.startsWith("/"),
}));

import { preparePrompt } from "./promptPreparation";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

beforeEach(() => {
  mocks.events.length = 0;
  mocks.applyFileMentionsToTurn.mockReset();
  mocks.applyNotesToTurn.mockReset();
  mocks.applyNotesToTurn.mockImplementation(async (text: string) => text);
  mocks.applySkillsToTurn.mockReset();
  mocks.warmNativeSkills.mockReset();
  mocks.warmNativeSkills.mockImplementation(() => {
    mocks.events.push("warm");
  });
});

describe("preparePrompt", () => {
  it.each([
    "/workflow foo @README.md",
    "/Review_Code a:b",
    "/omp:compact custom instructions",
  ])("preserves native command arguments: %s", async (text) => {
    await expect(
      preparePrompt(text, { harness: "omp", cwd: "/repo" }),
    ).resolves.toBe(text.replace("/omp:compact", "/compact"));
    expect(mocks.applyFileMentionsToTurn).not.toHaveBeenCalled();
    expect(mocks.applyNotesToTurn).not.toHaveBeenCalled();
    expect(mocks.applySkillsToTurn).not.toHaveBeenCalled();
  });
  it("starts warmup before awaiting file mentions", async () => {
    const files = deferred<string>();
    mocks.applyFileMentionsToTurn.mockImplementation(() => {
      mocks.events.push("files");
      return files.promise;
    });
    mocks.applySkillsToTurn.mockResolvedValue("prepared");

    const preparation = preparePrompt("hello", {
      harness: "pi",
      cwd: "/repo",
    });
    expect(mocks.events).toEqual(["warm", "files"]);

    files.resolve("with files");
    await expect(preparation).resolves.toBe("prepared");
    expect(mocks.applyNotesToTurn).toHaveBeenCalledWith("with files");
    expect(mocks.applySkillsToTurn).toHaveBeenCalledWith("with files", {
      harness: "pi",
      cwd: "/repo",
    });
  });
});

import { describe, expect, it, vi } from "vitest";
import { abortTextPromptRace, btwTextCancelled } from "./abortTextPrompt";

describe("abortTextPromptRace", () => {
  it("rejects with a BTW cancellation error and runs the abort hook", async () => {
    const controller = new AbortController();
    const onAbort = vi.fn();
    const race = abortTextPromptRace(controller.signal, onAbort);
    controller.abort();
    await expect(race.promise).rejects.toEqual(btwTextCancelled());
    expect(onAbort).toHaveBeenCalledOnce();
    race.detach();
  });
});

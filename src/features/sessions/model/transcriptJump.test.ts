import { expect, it, vi } from "vitest";
import {
  clearTranscriptJump,
  peekTranscriptJump,
  requestTranscriptJump,
  subscribeTranscriptJump,
} from "./transcriptJump";

it("keeps the newest requested message until its navigation completes", () => {
  const listener = vi.fn();
  const unsubscribe = subscribeTranscriptJump(listener);
  requestTranscriptJump("search-test", "old");
  const old = peekTranscriptJump("search-test")!;
  requestTranscriptJump("search-test", "new");
  const latest = peekTranscriptJump("search-test")!;

  clearTranscriptJump("search-test", old.token);
  expect(peekTranscriptJump("search-test")).toEqual(latest);
  clearTranscriptJump("search-test", latest.token);
  expect(peekTranscriptJump("search-test")).toBeNull();
  expect(listener).toHaveBeenCalledTimes(3);
  unsubscribe();
});

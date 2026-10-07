import { expect, it } from "vitest";
import {
  defaultMonoName,
  monoMessagePage,
  monoMessages,
  monoReply,
  monoState,
} from "./mono";
import type { Block } from "../../sessions/model/session";

it("is idle between turns", () => {
  expect(monoState({ blocks: [], busy: false })).toEqual({
    status: "idle",
  });
});

it("needs the user when a provider usage limit stops the conversation", () => {
  expect(monoState({ blocks: [], busy: false, usageLimit: { resetsAt: 1000 } }))
    .toEqual({ status: "needs-you", activity: "Usage limit reached" });
  expect(monoState({ blocks: [], busy: false, worktreeRemoved: true, usageLimit: {} }))
    .toEqual({ status: "idle" });
});

it("names the step it is on while working", () => {
  expect(
    monoState({
      busy: true,
      blocks: [
        {
          id: "1",
          role: "assistant",
          text: "",
          tool: { title: "Read old.ts" },
        },
        { id: "2", role: "user", text: "check CI" },
        {
          id: "3",
          role: "assistant",
          text: "",
          tool: { title: "Bash: `gh pr checks`" },
        },
      ],
    }),
  ).toEqual({ status: "working", activity: "Bash: gh pr checks" });
});

it("is thinking before the current turn has a step", () => {
  expect(
    monoState({
      busy: true,
      blocks: [
        {
          id: "1",
          role: "assistant",
          text: "",
          tool: { title: "Read old.ts" },
        },
        { id: "2", role: "user", text: "check CI" },
      ],
    }),
  ).toEqual({ status: "working", activity: "Thinking" });
});

it("says what it needs from you", () => {
  expect(
    monoState({
      busy: true,
      blocks: [
        {
          id: "1",
          role: "assistant",
          text: "",
          tool: { title: "git push" },
          approval: { requestId: 1 },
        },
      ],
    }),
  ).toEqual({ status: "needs-you", activity: "Approve git push" });
});

it("is idle when no turn is running", () => {
  expect(monoState({ blocks: [], busy: false })).toEqual({
    status: "idle",
  });
});

it("reports the current turn's step while working", () => {
  expect(
    monoState({
      busy: true,
      blocks: [
        { id: "0", role: "assistant", text: "", tool: { title: "Old step" } },
        { id: "1", role: "user", text: "check CI" },
        {
          id: "2",
          role: "assistant",
          text: "",
          tool: { title: "`gh pr checks 688`" },
        },
      ],
    }),
  ).toEqual({ status: "working", activity: "gh pr checks 688" });
  expect(
    monoState({
      busy: true,
      blocks: [
        { id: "0", role: "assistant", text: "", tool: { title: "Old step" } },
        { id: "1", role: "user", text: "hi" },
      ],
    }),
  ).toEqual({ status: "working", activity: "Thinking" });
});

it("says what it needs from you", () => {
  expect(
    monoState({
      busy: true,
      blocks: [
        {
          id: "1",
          role: "assistant",
          text: "",
          tool: { title: "git push" },
          approval: { requestId: 1 },
        },
      ],
    }),
  ).toEqual({ status: "needs-you", activity: "Approve git push" });
});

it("names an unnamed Mono after its mascot", () => {
  expect(defaultMonoName("crab")).toBe("MonoCrab");
  expect(defaultMonoName("mushroom")).toBe("MonoShroom");
});

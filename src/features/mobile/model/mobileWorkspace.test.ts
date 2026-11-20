import { describe, expect, it, vi } from "vitest";
import { newSession } from "../../sessions/model/session";
import { mobileConversation, mobileQuestionReply } from "./mobileWorkspace";

const storage = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: storage.invoke }));

const prompt = {
  requestId: 1,
  questions: [
    {
      id: "colour",
      prompt: "Which colour?",
      multiSelect: false,
      allowCustom: true,
      options: [
        { id: "red", label: "Red" },
        { id: "blue", label: "Blue" },
      ],
    },
    {
      id: "size",
      prompt: "Which sizes?",
      multiSelect: true,
      allowCustom: false,
      options: [
        { id: "s", label: "Small" },
        { id: "l", label: "Large" },
      ],
    },
  ],
};

describe("mobile question replies", () => {
  it("keeps the questions and options asked, and a deliberate partial answer", () => {
    expect(
      mobileQuestionReply(
        { kind: "answered", answers: { colour: ["red"], size: ["s", "xl"], other: ["x"] } },
        prompt,
      ),
    ).toEqual({ kind: "answered", answers: { colour: ["red"], size: ["s"] } });
    expect(
      mobileQuestionReply(
        { kind: "answered", answers: { colour: ["__custom__"] }, custom: { colour: " Teal " } },
        prompt,
      ),
    ).toEqual({ kind: "answered", answers: { colour: ["__custom__"] }, custom: { colour: "Teal" } });
  });

  it("skips only when asked to, rejecting answers that leave nothing answered", () => {
    expect(mobileQuestionReply({ kind: "skipped" }, prompt)).toEqual({ kind: "skipped" });
    expect(mobileQuestionReply({ kind: "answered", answers: { colour: ["green"] } }, prompt)).toBeUndefined();
    expect(
      mobileQuestionReply(
        { kind: "answered", answers: { colour: ["__custom__"] }, custom: { colour: "x".repeat(10_001) } },
        prompt,
      ),
    ).toBeUndefined();
    expect(mobileQuestionReply({ kind: "maybe" }, prompt)).toBeUndefined();
    expect(mobileQuestionReply(null, prompt)).toBeUndefined();
  });
});

it("hydrates saved tool approvals as history, while live sessions remain answerable", async () => {
  const record = newSession("pi", "/tmp/project");
  record.id = "saved-approval";
  record.blocks = [
    { id: "user", role: "user", text: "Inspect" },
    { id: "tool", role: "tool", text: "Run command", tool: { title: "git status", kind: "shell" }, approval: { requestId: 7 } },
    { id: "standalone", role: "approval", text: "Approve?", approval: { requestId: 8 } },
    { id: "decided", role: "tool", text: "Done", approval: { requestId: 9, decided: "allow" } },
  ];
  storage.invoke.mockResolvedValue(record);
  const saved = new Map<string, typeof record>();
  const first = await mobileConversation(record.id, [], saved);
  expect(first.approvals).toEqual([]);
  const hydrated = saved.get(record.id)!;
  expect(hydrated.blocks.find((block) => block.id === "tool")).toMatchObject({ tool: { title: "git status" } });
  expect(hydrated.blocks.find((block) => block.id === "tool")!.approval).toBeUndefined();
  expect(hydrated.blocks.find((block) => block.id === "standalone")).toBeUndefined();
  expect(hydrated.blocks.find((block) => block.id === "decided")!.approval?.decided).toBe("allow");
  const calls = storage.invoke.mock.calls.length;
  expect((await mobileConversation(record.id, [], saved)).approvals).toEqual([]);
  expect(storage.invoke).toHaveBeenCalledTimes(calls);
  const live = await mobileConversation(record.id, [{ ...record, busy: true }], saved);
  expect(live.approvals.map((approval) => approval.requestId)).toEqual([7, 8]);
});

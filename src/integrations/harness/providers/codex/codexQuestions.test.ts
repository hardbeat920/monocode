import { describe, expect, it } from "vitest";
import { CUSTOM_OPTION_ID } from "../../../../features/sessions/model/userQuestion";
import {
  codexAsyncQuestions,
  codexAsyncQuestionResponse,
  codexQuestions,
  codexQuestionResponse,
} from "./codexQuestions";

describe("Codex async question messages", () => {
  it("converts suggested choices and free-text questions with distinct IDs", () => {
    const questions = codexAsyncQuestions({
      type: "agentMessage",
      delivery: "async",
      questions: [
        { title: "Which scope?", options: ["Workspace", "Folder"] },
        { title: "Which scope?", options: null },
      ],
    });
    expect(questions).toMatchObject([
      {
        id: "q1",
        prompt: "Which scope?",
        allowCustom: true,
        options: [{ label: "Workspace" }, { label: "Folder" }],
      },
      { id: "q2", prompt: "Which scope?", allowCustom: true, options: [] },
    ]);
    expect(
      codexAsyncQuestionResponse(questions, {
        kind: "answered",
        answers: { q1: ["Folder"] },
        custom: { q2: "Selected files" },
      }),
    ).toBe("Which scope?\nFolder\n\nWhich scope?\nSelected files");
  });

  it("sends custom text instead of the Other option and omits skipped questions", () => {
    const questions = codexAsyncQuestions({
      type: "agentMessage",
      delivery: "async",
      questions: [
        { title: "Choose a source", options: ["Local"] },
        { title: "Anything else?", options: [] },
      ],
    });
    expect(
      codexAsyncQuestionResponse(questions, {
        kind: "answered",
        answers: { q1: [CUSTOM_OPTION_ID] },
        custom: { q1: "External source" },
      }),
    ).toBe("Choose a source\nExternal source");
    expect(codexAsyncQuestionResponse(questions, { kind: "skipped" })).toBe("");
  });

  it.each([
    { type: "agentMessage", questions: [{ title: "Ordinary message" }] },
    { type: "agentMessage", delivery: "async", questions: null },
    { type: "agentMessage", delivery: "async", questions: [] },
    { type: "reasoning", delivery: "async", questions: [{ title: "Why?" }] },
  ])("ignores items without async questions", (item) => {
    expect(codexAsyncQuestions(item)).toEqual([]);
  });

  it.each([
    {},
    [{ title: " " }],
    [{ title: "Choice", options: {} }],
    [{ title: "Choice", options: [{ label: "Local" }] }],
    [{ title: "Choice", options: [" "] }],
  ])("rejects malformed async questions", (questions) => {
    expect(() =>
      codexAsyncQuestions({
        type: "agentMessage",
        delivery: "async",
        questions,
      }),
    ).toThrow();
  });
});

describe("Codex question protocol", () => {
  it("returns selected labels and free text under the provider's IDs", () => {
    const questions = codexQuestions({
      questions: [
        {
          id: "scope",
          question: "Which scope?",
          isOther: true,
          options: [{ label: "Workspace" }],
        },
        { id: "reason", question: "Why?", isOther: false, options: null },
        { id: "skipped", question: "Anything else?", options: null },
      ],
    });
    expect(
      codexQuestionResponse(questions, {
        kind: "answered",
        answers: { scope: [CUSTOM_OPTION_ID] },
        custom: { scope: "Selected folder", reason: "Read docs" },
      }),
    ).toEqual({
      answers: {
        scope: { answers: ["Selected folder"] },
        reason: { answers: ["Read docs"] },
      },
    });
  });

  it("does not offer an extra free-text choice for a closed question", () => {
    const [question] = codexQuestions({
      questions: [
        {
          id: "q",
          question: "Continue?",
          isOther: false,
          options: [{ label: "Yes" }, { label: "No" }],
        },
      ],
    });
    expect(question.allowCustom).toBe(false);
    expect(
      codexQuestionResponse([question], {
        kind: "answered",
        answers: { q: ["No"] },
      }),
    ).toEqual({ answers: { q: { answers: ["No"] } } });
  });

  it.each([
    { questions: [] },
    { questions: [{ question: "Missing ID" }] },
    {
      questions: [
        { id: "a", question: "One" },
        { id: "a", question: "Two" },
      ],
    },
    { questions: [{ id: " key ", question: "Invalid ID" }] },
    { questions: [{ id: "q", question: "Malformed choices", options: {} }] },
    {
      questions: [{ id: "q", question: "Missing choice label", options: [{}] }],
    },
    { questions: [{ id: "q", question: "Secret", isSecret: true }] },
  ])("rejects unsupported input without exposing a broken form", (input) => {
    expect(() => codexQuestions(input)).toThrow();
  });
});

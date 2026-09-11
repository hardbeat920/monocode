import { describe, expect, it } from "vitest";
import { CUSTOM_OPTION_ID } from "../userQuestion";
import { codexQuestions, codexQuestionResponse } from "./codexQuestions";

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

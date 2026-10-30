import { describe, expect, it } from "vitest";
import { mobileQuestionReply } from "./mobileRequest";
import { CUSTOM_OPTION_ID, type UserQuestion } from "../../sessions/model/userQuestion";

const questions: UserQuestion[] = [
  {
    id: "db",
    prompt: "Which database?",
    multiSelect: false,
    allowCustom: true,
    options: [
      { id: "pg", label: "Postgres" },
      { id: "sqlite", label: "SQLite" },
    ],
  },
  {
    id: "extras",
    prompt: "Extras?",
    multiSelect: true,
    allowCustom: false,
    options: [
      { id: "auth", label: "Auth" },
      { id: "cache", label: "Cache" },
    ],
  },
];

describe("mobile question replies", () => {
  it("keeps only the question's own IDs and options", () => {
    expect(
      mobileQuestionReply(questions, {
        answers: { db: ["pg", "mysql", 3], extras: ["auth", "auth", "cache"], nope: ["x"] },
        custom: { nope: "ignored" },
      }),
    ).toEqual({ kind: "answered", answers: { db: ["pg"], extras: ["auth", "cache"] } });
  });

  it("accepts a typed answer where the question allows one", () => {
    expect(
      mobileQuestionReply(questions, {
        answers: { db: [CUSTOM_OPTION_ID] },
        custom: { db: "  DuckDB  " },
      }),
    ).toEqual({ kind: "answered", answers: { db: [CUSTOM_OPTION_ID] }, custom: { db: "DuckDB" } });
  });

  it("skips only when asked, and refuses an empty answer", () => {
    expect(mobileQuestionReply(questions, { skip: true })).toEqual({ kind: "skipped" });
    expect(() => mobileQuestionReply(questions, { answers: { db: ["pg", "sqlite"] } })).toThrow();
    expect(() => mobileQuestionReply(questions, {})).toThrow();
  });
});

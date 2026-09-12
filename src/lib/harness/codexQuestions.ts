import {
  questionsFromUnknown,
  selectedAnswerLabels,
  type UserQuestion,
  type UserQuestionReply,
} from "../userQuestion";
import { asRecord } from "./codexProtocol";

export function codexQuestions(params: unknown): UserQuestion[] {
  const raw = asRecord(params)?.questions;
  if (!Array.isArray(raw) || raw.length === 0) {
    throw new Error("Codex sent an unsupported user-input request.");
  }
  // The shared question UI stores answers in the transcript and is not a
  // secure credential-entry surface. Never send secret questions to it.
  if (raw.some((question) => asRecord(question)?.isSecret === true)) {
    throw new Error(
      "Codex requested secret input. MonoCode cannot collect secret answers securely.",
    );
  }
  const questions = questionsFromUnknown({
    questions: raw.map((question) => {
      const rec = asRecord(question);
      if (
        !rec ||
        typeof rec.id !== "string" ||
        !rec.id.trim() ||
        rec.id !== rec.id.trim() ||
        typeof rec.question !== "string" ||
        !rec.question.trim() ||
        (rec.options != null &&
          (!Array.isArray(rec.options) ||
            rec.options.some((option) => {
              const label = asRecord(option)?.label;
              return typeof label !== "string" || !label.trim();
            })))
      ) {
        throw new Error("Codex sent an unsupported user-input question.");
      }
      return {
        ...rec,
        allowCustom:
          rec.isOther === true ||
          rec.options == null ||
          (Array.isArray(rec.options) && rec.options.length === 0),
      };
    }),
  });
  if (new Set(raw.map((q) => asRecord(q)?.id)).size !== raw.length) {
    throw new Error("Codex sent duplicate user-input question IDs.");
  }
  return questions;
}

export function codexQuestionResponse(
  questions: UserQuestion[],
  reply: UserQuestionReply,
): { answers: Record<string, { answers: string[] }> } {
  if (reply.kind === "skipped") return { answers: {} };
  return {
    answers: Object.fromEntries(
      questions.flatMap((question) => {
        const answers = selectedAnswerLabels(question, reply);
        return answers.length ? [[question.id, { answers }]] : [];
      }),
    ),
  };
}

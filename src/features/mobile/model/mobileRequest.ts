import { pendingApprovalForSession } from "../../notifications/model/approvalToast";
import type { Session } from "../../sessions/model/session";
import {
  buildQuestionReply,
  CUSTOM_OPTION_ID,
  type UserQuestion,
  type UserQuestionReply,
} from "../../sessions/model/userQuestion";

/** What a session waits on the user for, with what the phone needs to answer it. */
export function mobileRequest(session: Session) {
  const pending = pendingApprovalForSession(session);
  if (!pending) return {};
  const question = session.pendingQuestion;
  return {
    waiting: pending.kind,
    request:
      pending.kind === "question" && question
        ? {
            kind: "question" as const,
            requestId: question.requestId,
            title: question.title,
            questions: question.questions,
          }
        : {
            kind: "approval" as const,
            requestId: pending.requestId,
            label: pending.label,
          },
  };
}

const CUSTOM_MAX = 10_000;
const asRecord = (value: unknown) =>
  value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;

/** The phone's answers to a question, kept to that question's own IDs and options. */
export function mobileQuestionReply(
  questions: UserQuestion[],
  params: Record<string, unknown>,
): UserQuestionReply {
  if (params.skip === true) return { kind: "skipped" };
  const picked = asRecord(params.answers);
  const typed = asRecord(params.custom);
  const answers: Record<string, string[]> = {};
  const custom: Record<string, string> = {};
  for (const question of questions) {
    const ids = picked?.[question.id];
    if (Array.isArray(ids))
      answers[question.id] = [
        ...new Set(
          ids.filter(
            (id): id is string =>
              id === CUSTOM_OPTION_ID ||
              question.options.some((option) => option.id === id),
          ),
        ),
      ];
    const text = typed?.[question.id];
    if (typeof text === "string" && text.length <= CUSTOM_MAX)
      custom[question.id] = text;
  }
  const reply = buildQuestionReply(questions, answers, custom);
  if (reply.kind === "skipped")
    throw new Error("Answer at least one question, or skip");
  return reply;
}

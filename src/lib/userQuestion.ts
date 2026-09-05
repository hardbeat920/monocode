import { t } from "./i18n";

/** Shared clarifying-question model for every harness that can ask the user. */

export const CUSTOM_OPTION_ID = "__custom__";

export type UserQuestionOption = {
  id: string;
  label: string;
  description?: string;
};

export type UserQuestion = {
  id: string;
  header?: string;
  prompt: string;
  multiSelect: boolean;
  allowCustom: boolean;
  options: UserQuestionOption[];
};

export type UserQuestionPrompt = {
  requestId: number;
  title?: string;
  questions: UserQuestion[];
};

export type UserQuestionReply =
  | {
      kind: "answered";
      answers: Record<string, string[]>;
      custom?: Record<string, string>;
    }
  | { kind: "skipped" };

export function questionsFromUnknown(value: unknown): UserQuestion[] {
  const rec = asRecord(value);
  const nested =
    asRecord(rec?.input) ?? asRecord(rec?.params) ?? asRecord(rec?.question);
  const raw = firstArray(
    rec?.questions,
    nested?.questions,
    Array.isArray(value) ? value : undefined,
  );
  const usedIds = new Set<string>();
  return raw.flatMap((item, index) => {
    const parsed = questionFromUnknown(item, index, usedIds);
    return parsed ? [parsed] : [];
  });
}

export function questionPromptTitle(questions: UserQuestion[]): string {
  const first = questions[0];
  return first?.header || first?.prompt || t("Question");
}

export function questionIsComplete(
  question: UserQuestion,
  answers: Record<string, string[]>,
  custom: Record<string, string> = {},
): boolean {
  const selected = answers[question.id] ?? [];
  if (selected.length === 0) {
    return question.allowCustom && !!custom[question.id]?.trim();
  }
  if (!question.multiSelect && selected.length !== 1) return false;
  return selected.every((id) => {
    if (!isCustomSelection(question, id)) return true;
    return !!custom[question.id]?.trim();
  });
}

export function questionAnswersComplete(
  questions: UserQuestion[],
  answers: Record<string, string[]>,
  custom: Record<string, string> = {},
): boolean {
  if (questions.length === 0) return false;
  return questions.every((question) =>
    questionIsComplete(question, answers, custom),
  );
}

/** Skip every question → skipped. Any answered question → answered with those only. */
export function buildQuestionReply(
  questions: UserQuestion[],
  answers: Record<string, string[]>,
  custom: Record<string, string> = {},
): UserQuestionReply {
  const answered = questions.filter((question) =>
    questionIsComplete(question, answers, custom),
  );
  if (answered.length === 0) return { kind: "skipped" };
  const nextAnswers: Record<string, string[]> = {};
  const nextCustom: Record<string, string> = {};
  for (const question of answered) {
    const selected = answers[question.id];
    if (selected?.length) nextAnswers[question.id] = selected;
    const text = custom[question.id]?.trim();
    if (text) nextCustom[question.id] = text;
  }
  return {
    kind: "answered",
    answers: nextAnswers,
    ...(Object.keys(nextCustom).length > 0 ? { custom: nextCustom } : {}),
  };
}

export function selectedAnswerLabels(
  question: UserQuestion,
  reply: Extract<UserQuestionReply, { kind: "answered" }>,
): string[] {
  const custom = reply.custom?.[question.id]?.trim();
  const selected = reply.answers[question.id] ?? [];
  if (selected.length === 0 && custom && question.allowCustom) return [custom];
  return selected.flatMap((id) => {
    if (isCustomSelection(question, id)) return custom ? [custom] : [];
    const option = question.options.find((item) => item.id === id);
    const label = option?.label.trim() || id.trim();
    return label ? [label] : [];
  });
}

export function isCustomSelection(
  question: UserQuestion,
  optionId: string,
): boolean {
  if (optionId === CUSTOM_OPTION_ID) return true;
  const option = question.options.find((item) => item.id === optionId);
  return option ? isOtherOption(option) : false;
}

export function isOtherOption(option: UserQuestionOption): boolean {
  return option.id === CUSTOM_OPTION_ID || /^other$/i.test(option.label.trim());
}

function questionFromUnknown(
  value: unknown,
  index: number,
  usedIds: Set<string>,
): UserQuestion | null {
  const rec = asRecord(value);
  if (!rec) return null;
  const prompt =
    stringField(rec, "question") ??
    stringField(rec, "prompt") ??
    stringField(rec, "text") ??
    stringField(rec, "header");
  const options = optionsFromUnknown(rec.options);
  const allowCustom = customAllowed(rec, options);
  if (!prompt && options.length === 0 && !allowCustom) return null;
  const header = stringField(rec, "header") ?? stringField(rec, "title");
  return {
    id: uniqueId(
      stringField(rec, "id") ?? prompt ?? header ?? `q${index + 1}`,
      usedIds,
    ),
    ...(header ? { header } : {}),
    prompt: prompt || header || t("Question {0}", [index + 1]),
    multiSelect:
      rec.multiSelect === true ||
      rec.allowMultiple === true ||
      rec.multiple === true,
    allowCustom,
    options,
  };
}

function optionsFromUnknown(value: unknown): UserQuestionOption[] {
  if (!Array.isArray(value)) return [];
  const usedIds = new Set<string>();
  return value.flatMap((item) => {
    if (typeof item === "string" && item.trim()) {
      const label = item.trim();
      return [{ id: uniqueId(label, usedIds), label }];
    }
    const rec = asRecord(item);
    if (!rec) return [];
    const label =
      stringField(rec, "label") ??
      stringField(rec, "value") ??
      stringField(rec, "text") ??
      stringField(rec, "id");
    if (!label) return [];
    const description = stringField(rec, "description") ?? stringField(rec, "detail");
    return [
      {
        id: uniqueId(
          stringField(rec, "id") ??
            stringField(rec, "optionId") ??
            stringField(rec, "value") ??
            label,
          usedIds,
        ),
        label,
        ...(description ? { description } : {}),
      },
    ];
  });
}

function customAllowed(
  rec: Record<string, unknown>,
  options: UserQuestionOption[],
): boolean {
  if (typeof rec.custom === "boolean") return rec.custom;
  if (typeof rec.allowCustom === "boolean") return rec.allowCustom;
  if (options.some(isOtherOption)) return true;
  // Cursor-style options carry stable ids; Claude/OpenCode/Grok add Other in the UI.
  if (stringField(rec, "prompt") && !stringField(rec, "question")) return false;
  return true;
}

function uniqueId(seed: string, used: Set<string>): string {
  const base = seed.trim() || "option";
  let next = base;
  let n = 2;
  while (used.has(next)) {
    next = `${base}:${n}`;
    n += 1;
  }
  used.add(next);
  return next;
}

function firstArray(...values: unknown[]): unknown[] {
  for (const value of values) {
    if (Array.isArray(value)) return value;
  }
  return [];
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

function stringField(
  rec: Record<string, unknown> | null | undefined,
  key: string,
): string | undefined {
  if (!rec) return undefined;
  const value = rec[key];
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

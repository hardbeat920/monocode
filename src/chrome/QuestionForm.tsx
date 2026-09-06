import { useEffect, useMemo, useState } from "react";
import { Check, MessageSquare } from "./icons";
import { t } from "../lib/i18n";
import {
  CUSTOM_OPTION_ID,
  buildQuestionReply,
  isOtherOption,
  questionIsComplete,
  type UserQuestion,
  type UserQuestionPrompt,
  type UserQuestionReply,
} from "../lib/userQuestion";

type Props = {
  prompt: UserQuestionPrompt;
  onReply: (requestId: number, reply: UserQuestionReply) => void;
};

export function QuestionForm({ prompt, onReply }: Props) {
  const [step, setStep] = useState(0);
  const [answers, setAnswers] = useState<Record<string, string[]>>({});
  const [custom, setCustom] = useState<Record<string, string>>({});

  useEffect(() => {
    setStep(0);
    setAnswers({});
    setCustom({});
  }, [prompt.requestId]);

  const questions = prompt.questions;
  const total = questions.length;
  const index = Math.min(step, Math.max(total - 1, 0));
  const question = questions[index];
  const last = index >= total - 1;
  const ready = useMemo(
    () => (question ? questionIsComplete(question, answers, custom) : false),
    [answers, custom, question],
  );

  const finish = (nextAnswers = answers, nextCustom = custom) => {
    onReply(prompt.requestId, buildQuestionReply(questions, nextAnswers, nextCustom));
  };

  const skipCurrent = () => {
    if (!question) {
      finish();
      return;
    }
    const nextAnswers = { ...answers };
    const nextCustom = { ...custom };
    delete nextAnswers[question.id];
    delete nextCustom[question.id];
    if (last) {
      finish(nextAnswers, nextCustom);
      return;
    }
    setAnswers(nextAnswers);
    setCustom(nextCustom);
    setStep(index + 1);
  };

  const continueCurrent = () => {
    if (!question || !ready) return;
    if (last) {
      finish();
      return;
    }
    setStep(index + 1);
  };

  if (!question) return null;

  const title = question.header?.trim() || prompt.title?.trim() || t("Question");

  return (
    <div className="px-1.5 pb-1.5" data-question-form>
      <form
        className="rounded-lg border border-content/10 bg-content/3 px-3 py-2.5"
        onSubmit={(event) => {
          event.preventDefault();
          continueCurrent();
        }}
      >
        <div className="flex items-center gap-1.5">
          <MessageSquare
            className="size-3.5 shrink-0 text-content/45"
            strokeWidth={1.75}
          />
          <span className="min-w-0 flex-1 truncate text-[11px] text-content/50">
            {title}
          </span>
          {total > 1 ? (
            <span className="shrink-0 text-[11px] text-content/40">
              {t("{0} of {1}", [String(index + 1), String(total)])}
            </span>
          ) : null}
          <button
            type="button"
            className="h-6 shrink-0 rounded-md px-1.5 text-[11px] text-content/55 hover:bg-content/10 hover:text-content"
            onClick={skipCurrent}
          >
            {t("Skip")}
          </button>
        </div>
        <div className="mt-2">
          <QuestionFields
            key={question.id}
            question={question}
            selected={answers[question.id] ?? []}
            custom={custom[question.id] ?? ""}
            onSelect={(optionId) =>
              setAnswers((current) => ({
                ...current,
                [question.id]: nextSelection(
                  question,
                  current[question.id] ?? [],
                  optionId,
                ),
              }))
            }
            onCustom={(value) => {
              setCustom((current) => ({ ...current, [question.id]: value }));
              setAnswers((current) => {
                const selected = current[question.id] ?? [];
                if (question.multiSelect) {
                  const without = selected.filter(
                    (id) => !isCustomId(question, id),
                  );
                  return {
                    ...current,
                    [question.id]: [...without, customOptionId(question)],
                  };
                }
                return {
                  ...current,
                  [question.id]: [customOptionId(question)],
                };
              });
            }}
          />
        </div>
        <div className="mt-2.5 flex justify-end">
          <button
            type="submit"
            disabled={!ready}
            className="h-6 rounded-md bg-content px-2.5 text-[11px] font-medium text-background-base hover:bg-content/80 disabled:opacity-40"
          >
            {t("Continue")}
          </button>
        </div>
      </form>
    </div>
  );
}

function QuestionFields({
  question,
  selected,
  custom,
  onSelect,
  onCustom,
}: {
  question: UserQuestion;
  selected: string[];
  custom: string;
  onSelect: (optionId: string) => void;
  onCustom: (value: string) => void;
}) {
  const options = displayOptions(question);
  const customSelected = selected.some((id) => isCustomId(question, id));
  const customId = customOptionId(question);

  return (
    <fieldset className="min-w-0" aria-label={question.header || question.prompt}>
      <p className="text-[13px] font-medium leading-snug text-content">
        {question.prompt}
      </p>
      {question.multiSelect ? (
        <p className="mt-0.5 text-[11px] text-content/40">{t("Select all that apply")}</p>
      ) : null}
      {options.length === 0 && question.allowCustom ? (
        <input
          value={custom}
          onChange={(event) => onCustom(event.target.value)}
          placeholder={t("Type your answer")}
          className="mt-1.5 w-full rounded-md border border-content/15 bg-transparent px-2 py-1 text-[12px] text-content outline-none placeholder:text-content/35 focus:border-content/30"
        />
      ) : (
        <div className="mt-1.5 flex max-h-52 flex-col gap-1 overflow-y-auto" role="group">
          {options.map((option) => {
            const isCustom =
              isOtherOption(option) || option.id === CUSTOM_OPTION_ID;
            const active = selected.includes(option.id);
            return (
              <div key={option.id}>
                <button
                  type="button"
                  aria-pressed={active}
                  onClick={() => onSelect(option.id)}
                  className={`flex w-full items-start gap-2 rounded-md border px-2 py-1.5 text-left ${
                    active
                      ? "border-content/35 bg-content/10"
                      : "border-content/10 hover:bg-content/5"
                  }`}
                >
                  <span
                    aria-hidden
                    className={`mt-0.5 grid size-3.5 shrink-0 place-items-center border ${
                      question.multiSelect ? "rounded-[3px]" : "rounded-full"
                    } ${
                      active
                        ? "border-content bg-content text-background-base"
                        : "border-content/30"
                    }`}
                  >
                    {active ? (
                      <Check className="size-2.5" strokeWidth={2.5} />
                    ) : null}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block text-[12px] leading-snug text-content">
                      {option.label}
                    </span>
                    {option.description ? (
                      <span className="mt-0.5 block text-[11px] leading-snug text-content/50">
                        {option.description}
                      </span>
                    ) : null}
                  </span>
                </button>
                {isCustom && (active || customSelected) ? (
                  <input
                    value={custom}
                    onChange={(event) => onCustom(event.target.value)}
                    placeholder={t("Type your answer")}
                    className="mt-1 w-full rounded-md border border-content/15 bg-transparent px-2 py-1 text-[12px] text-content outline-none placeholder:text-content/35 focus:border-content/30"
                    onClick={(event) => event.stopPropagation()}
                    onFocus={() => {
                      if (!customSelected) onSelect(customId);
                    }}
                  />
                ) : null}
              </div>
            );
          })}
        </div>
      )}
    </fieldset>
  );
}

function displayOptions(question: UserQuestion): UserQuestion["options"] {
  if (question.options.length === 0) return question.options;
  if (question.options.some(isOtherOption) || !question.allowCustom) {
    return question.options;
  }
  return [...question.options, { id: CUSTOM_OPTION_ID, label: t("Other") }];
}

function customOptionId(question: UserQuestion): string {
  return question.options.find(isOtherOption)?.id ?? CUSTOM_OPTION_ID;
}

function isCustomId(question: UserQuestion, optionId: string): boolean {
  return optionId === CUSTOM_OPTION_ID || optionId === customOptionId(question);
}

function nextSelection(
  question: UserQuestion,
  current: string[],
  optionId: string,
): string[] {
  if (!question.multiSelect) return [optionId];
  if (current.includes(optionId)) {
    return current.filter((id) => id !== optionId);
  }
  if (isCustomId(question, optionId)) {
    return [...current.filter((id) => !isCustomId(question, id)), optionId];
  }
  return [...current, optionId];
}

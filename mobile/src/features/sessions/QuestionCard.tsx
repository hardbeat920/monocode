import { useEffect, useRef, useState } from "react";
import {
  Alert,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  useWindowDimensions,
  View,
} from "react-native";
import Animated, { FadeIn, FadeOut } from "react-native-reanimated";
import type { DisplayedQuestionPrompt, Question, QuestionReply } from "@/features/workspace/types";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { Icon } from "@/ui/Icon";
import { LiquidView } from "@/ui/LiquidView";
import { radii, themedStyles, useColors, useScheme } from "@/ui/theme";

// The desktop's CUSTOM_OPTION_ID (src/features/sessions/model/userQuestion.ts).
const CUSTOM_OPTION_ID = "__custom__";
/** As the composer card's inset, so the two line up. */
const MARGIN = 12;
/** The question and its answers scroll past this, so Skip and Send stay on
 *  screen with the composer and keyboard under them. */
const BODY_MAX = 320;
const BODY_SHARE = 0.35;

/**
 * What an agent asks, over the composer, as the desktop's QuestionForm shows
 * it: one question at a time, Skip moving past one and Continue answering it.
 * The last one sends every answer to the desktop. Touching a timed one keeps it
 * open, as using the desktop's does.
 */
export function QuestionCard({
  conversationId,
  prompt,
}: {
  conversationId: string;
  prompt: DisplayedQuestionPrompt;
}) {
  // A clear and replacement may be batched without ever rendering an empty card.
  return <QuestionForm key={`${conversationId}:${prompt.displayId}`} conversationId={conversationId} prompt={prompt} />;
}

function QuestionForm({ conversationId, prompt }: {
  conversationId: string;
  prompt: DisplayedQuestionPrompt;
}) {
  const styles = useStyles();
  const colors = useColors();
  const { height } = useWindowDimensions();
  const { answerQuestion, keepQuestionOpen } = useWorkspace();
  const kept = useRef<{ conversationId: string; requestId: number } | null>(null);
  const [step, setStep] = useState(0);
  const [answers, setAnswers] = useState<Record<string, string[]>>({});
  const [custom, setCustom] = useState<Record<string, string>>({});
  const [sending, setSending] = useState(false);
  const now = useNow(prompt.autoResolveAt != null);

  const questions = prompt.questions;
  const total = questions.length;
  const index = Math.min(step, Math.max(total - 1, 0));
  const question = questions[index];
  const last = index >= total - 1;
  if (!question) return null;
  const ready = isComplete(question, answers, custom);

  const finish = async (nextAnswers = answers, nextCustom = custom) => {
    setSending(true);
    try {
      await answerQuestion(conversationId, prompt.requestId, buildReply(questions, nextAnswers, nextCustom));
    } catch (reason) {
      Alert.alert("Answer not sent", reason instanceof Error ? reason.message : String(reason));
    } finally {
      setSending(false);
    }
  };
  const skip = () => {
    const nextAnswers = { ...answers };
    const nextCustom = { ...custom };
    delete nextAnswers[question.id];
    delete nextCustom[question.id];
    setAnswers(nextAnswers);
    setCustom(nextCustom);
    if (last) void finish(nextAnswers, nextCustom);
    else setStep(index + 1);
  };
  const next = () => {
    if (!ready) return;
    if (last) void finish();
    else setStep(index + 1);
  };

  const title = question.header?.trim() || prompt.title?.trim() || "Question";
  const options = displayOptions(question);
  const selected = answers[question.id] ?? [];
  const customId = customOptionId(question);
  const customSelected = selected.some((id) => isCustomId(question, id));
  const select = (optionId: string) =>
    setAnswers((current) => ({
      ...current,
      [question.id]: nextSelection(question, current[question.id] ?? [], optionId),
    }));
  const type = (value: string) => {
    setCustom((current) => ({ ...current, [question.id]: value }));
    setAnswers((current) => {
      const chosen = current[question.id] ?? [];
      return {
        ...current,
        [question.id]: question.multiSelect
          ? [...chosen.filter((id) => !isCustomId(question, id)), customId]
          : [customId],
      };
    });
  };
  const customInput = (
    <CustomInput value={custom[question.id] ?? ""} onChange={type} />
  );

  const interact = async () => {
    if (prompt.autoResolveAt == null || (
      kept.current?.conversationId === conversationId && kept.current.requestId === prompt.requestId
    )) return;
    const attempt = { conversationId, requestId: prompt.requestId };
    kept.current = attempt;
    try {
      await keepQuestionOpen(conversationId, prompt.requestId);
    } catch {
      // A later touch can retry; never duplicate a request still in flight.
      if (kept.current === attempt) kept.current = null;
    }
  };

  return (
    <Animated.View
      entering={FadeIn.duration(180)}
      exiting={FadeOut.duration(180)}
      onTouchStart={interact}
      style={styles.card}
    >
      <LiquidView fill row={false} borderRadius={radii.card} style={StyleSheet.absoluteFill}>
        <View />
      </LiquidView>
      <View style={styles.header}>
        <Icon name="chat" size={14} color={colors.muted} />
        <Text style={styles.title} numberOfLines={1}>
          {title}
        </Text>
        {total > 1 ? (
          <Text style={styles.meta}>
            {index + 1} of {total}
          </Text>
        ) : null}
        <TextButton label="Skip" onPress={skip} disabled={sending} />
      </View>
      <ScrollView
        style={[styles.body, { maxHeight: Math.min(BODY_MAX, height * BODY_SHARE) }]}
        contentContainerStyle={styles.bodyContent}
        keyboardShouldPersistTaps="handled"
      >
        <Text style={styles.prompt}>{question.prompt}</Text>
        {question.multiSelect ? <Text style={styles.meta}>Select all that apply</Text> : null}
        {options.length === 0 && question.allowCustom ? (
          customInput
        ) : (
          options.map((option) => {
            const active = selected.includes(option.id);
            const other = isCustomId(question, option.id);
            return (
              <View key={option.id}>
                <Pressable
                  accessibilityRole={question.multiSelect ? "checkbox" : "radio"}
                  accessibilityState={{ checked: active }}
                  accessibilityHint={option.description}
                  onPress={() => select(option.id)}
                  style={({ pressed }) => [
                    styles.option,
                    active && styles.optionActive,
                    pressed && styles.pressed,
                  ]}
                >
                  <View
                    style={[
                      styles.mark,
                      question.multiSelect ? styles.checkbox : styles.radio,
                      active && styles.markActive,
                    ]}
                  >
                    {active ? <Icon name="check" size={11} color={colors.inverse} /> : null}
                  </View>
                  <View style={styles.optionText}>
                    <Text style={styles.label}>{option.label}</Text>
                    {option.description ? (
                      <Text style={styles.description}>{option.description}</Text>
                    ) : null}
                  </View>
                </Pressable>
                {other && (active || customSelected) ? customInput : null}
              </View>
            );
          })
        )}
      </ScrollView>
      <View style={styles.footer}>
        {prompt.autoResolveAt != null ? (
          <Text style={[styles.meta, styles.countdown]} numberOfLines={1}>
            {prompt.autoResolveAt - now > 60_000
              ? "Optional question"
              : `Continues without an answer in ${Math.max(0, Math.ceil((prompt.autoResolveAt - now) / 1000))}s`}
          </Text>
        ) : null}
        {index > 0 ? (
          <TextButton label="Back" onPress={() => setStep(index - 1)} disabled={sending} />
        ) : null}
        <Pressable
          accessibilityRole="button"
          accessibilityState={{ disabled: !ready || sending }}
          disabled={!ready || sending}
          onPress={next}
          style={({ pressed }) => [
            styles.continue,
            (!ready || sending) && styles.disabled,
            pressed && styles.pressed,
          ]}
        >
          <Text style={styles.continueLabel}>{last ? "Send" : "Continue"}</Text>
        </Pressable>
      </View>
    </Animated.View>
  );
}

function CustomInput({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const styles = useStyles();
  const colors = useColors();
  const scheme = useScheme();
  return (
    <TextInput
      value={value}
      onChangeText={onChange}
      placeholder="Type your answer"
      placeholderTextColor={colors.muted}
      keyboardAppearance={scheme}
      cursorColor={colors.accent}
      selectionColor={colors.accent}
      multiline
      style={styles.input}
    />
  );
}

function TextButton({
  label,
  onPress,
  disabled,
}: {
  label: string;
  onPress: () => void;
  disabled?: boolean;
}) {
  const styles = useStyles();
  return (
    <Pressable
      accessibilityRole="button"
      disabled={disabled}
      onPress={onPress}
      hitSlop={8}
      style={({ pressed }) => [styles.textButton, (pressed || disabled) && styles.pressed]}
    >
      <Text style={styles.textButtonLabel}>{label}</Text>
    </Pressable>
  );
}

/** The clock, each second while a deadline counts down. */
function useNow(ticking: boolean) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (!ticking) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [ticking]);
  return now;
}

// As the desktop's userQuestion.ts and QuestionForm.tsx, so both answer alike.
function isOther(option: { id: string; label: string }) {
  return option.id === CUSTOM_OPTION_ID || /^other$/i.test(option.label.trim());
}

function displayOptions(question: Question): Question["options"] {
  if (question.options.length === 0) return question.options;
  if (question.options.some(isOther) || !question.allowCustom) return question.options;
  return [...question.options, { id: CUSTOM_OPTION_ID, label: "Other" }];
}

function customOptionId(question: Question) {
  return question.options.find(isOther)?.id ?? CUSTOM_OPTION_ID;
}

function isCustomId(question: Question, optionId: string) {
  return optionId === CUSTOM_OPTION_ID || optionId === customOptionId(question);
}

function nextSelection(question: Question, current: string[], optionId: string) {
  if (!question.multiSelect) return [optionId];
  if (current.includes(optionId)) return current.filter((id) => id !== optionId);
  if (isCustomId(question, optionId))
    return [...current.filter((id) => !isCustomId(question, id)), optionId];
  return [...current, optionId];
}

function isComplete(
  question: Question,
  answers: Record<string, string[]>,
  custom: Record<string, string>,
) {
  const selected = answers[question.id] ?? [];
  if (selected.length === 0) return question.allowCustom && !!custom[question.id]?.trim();
  if (!question.multiSelect && selected.length !== 1) return false;
  return selected.every((id) => !isCustomId(question, id) || !!custom[question.id]?.trim());
}

/** Skipping every question skips; otherwise only the answered ones go. */
function buildReply(
  questions: Question[],
  answers: Record<string, string[]>,
  custom: Record<string, string>,
): QuestionReply {
  const answered = questions.filter((question) => isComplete(question, answers, custom));
  if (answered.length === 0) return { kind: "skipped" };
  const chosen: Record<string, string[]> = {};
  const typed: Record<string, string> = {};
  for (const question of answered) {
    if (answers[question.id]?.length) chosen[question.id] = answers[question.id];
    const text = custom[question.id]?.trim();
    if (text) typed[question.id] = text;
  }
  return {
    kind: "answered",
    answers: chosen,
    ...(Object.keys(typed).length > 0 ? { custom: typed } : {}),
  };
}

const useStyles = themedStyles((colors) => ({
  card: {
    marginHorizontal: MARGIN,
    marginBottom: 8,
    paddingHorizontal: 16,
    paddingTop: 12,
    paddingBottom: 12,
    gap: 6,
  },
  header: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  title: {
    flex: 1,
    color: colors.muted,
    fontSize: 13,
  },
  meta: {
    color: colors.muted,
    fontSize: 12,
  },
  prompt: {
    color: colors.content,
    fontSize: 15,
    fontWeight: "500",
    lineHeight: 20,
  },
  body: {
    flexGrow: 0,
  },
  bodyContent: {
    gap: 6,
  },
  option: {
    flexDirection: "row",
    alignItems: "flex-start",
    gap: 10,
    paddingHorizontal: 12,
    paddingVertical: 10,
    borderRadius: radii.row,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: colors.stroke,
  },
  optionActive: {
    backgroundColor: colors.selection,
    borderColor: colors.muted,
  },
  mark: {
    width: 18,
    height: 18,
    marginTop: 1,
    alignItems: "center",
    justifyContent: "center",
    borderWidth: 1.5,
    borderColor: colors.faint,
  },
  radio: {
    borderRadius: 9,
  },
  checkbox: {
    borderRadius: 5,
  },
  markActive: {
    backgroundColor: colors.content,
    borderColor: colors.content,
  },
  optionText: {
    flex: 1,
    gap: 2,
  },
  label: {
    color: colors.content,
    fontSize: 15,
  },
  description: {
    color: colors.muted,
    fontSize: 13,
    lineHeight: 18,
  },
  input: {
    marginTop: 6,
    minHeight: 40,
    maxHeight: 120,
    paddingHorizontal: 12,
    paddingVertical: 10,
    borderRadius: radii.row,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: colors.stroke,
    color: colors.content,
    fontSize: 15,
  },
  footer: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "flex-end",
    gap: 16,
    marginTop: 6,
  },
  countdown: {
    flex: 1,
  },
  textButton: {
    paddingVertical: 4,
  },
  textButtonLabel: {
    color: colors.muted,
    fontSize: 14,
  },
  continue: {
    height: 34,
    paddingHorizontal: 16,
    borderRadius: radii.pill,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.content,
  },
  continueLabel: {
    color: colors.inverse,
    fontSize: 14,
    fontWeight: "600",
  },
  disabled: {
    opacity: 0.4,
  },
  pressed: {
    opacity: 0.6,
  },
}));

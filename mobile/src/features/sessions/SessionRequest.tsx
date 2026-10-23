import { useState } from "react";
import {
  Alert,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
  useWindowDimensions,
} from "react-native";

import type { RequestQuestion, RequestReply, SessionRequest } from "@/features/workspace/types";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { LiquidView } from "@/ui/LiquidView";
import { themedStyles, useColors, useScheme } from "@/ui/theme";

// The desktop's userQuestion.ts: an option labelled "Other", or this ID, takes typed text.
const CUSTOM_OPTION_ID = "__custom__";
const isOther = (option: RequestQuestion["options"][number]) =>
  option.id === CUSTOM_OPTION_ID || /^other$/i.test(option.label.trim());

const RADIUS = 25;

/**
 * What a session waits on the user for, answered over its composer as the
 * desktop's approval card and question form are. Key it by the request, so a
 * new one starts fresh.
 */
export function SessionRequestCard({
  sessionId,
  request,
}: {
  sessionId: string;
  request: SessionRequest;
}) {
  const styles = useStyles();
  const { height } = useWindowDimensions();
  const { respondToRequest } = useWorkspace();
  const [sending, setSending] = useState(false);
  // The card leaves once the desktop has the answer; it stays on a failure.
  const respond = async (reply: RequestReply) => {
    setSending(true);
    try {
      await respondToRequest(sessionId, reply);
    } catch (reason) {
      Alert.alert("Couldn't answer", reason instanceof Error ? reason.message : String(reason));
      setSending(false);
    }
  };
  return (
    <View style={styles.card}>
      <LiquidView fill row={false} borderRadius={RADIUS} style={StyleSheet.absoluteFill}>
        <View />
      </LiquidView>
      <ScrollView
        style={{ maxHeight: height * 0.45 }}
        contentContainerStyle={styles.content}
        keyboardShouldPersistTaps="handled"
      >
        {request.kind === "approval" ? (
          <Approval request={request} sending={sending} respond={respond} />
        ) : (
          <Questions request={request} sending={sending} respond={respond} />
        )}
      </ScrollView>
    </View>
  );
}

type Respond = (reply: RequestReply) => void;

function Approval({
  request,
  sending,
  respond,
}: {
  request: Extract<SessionRequest, { kind: "approval" }>;
  sending: boolean;
  respond: Respond;
}) {
  const styles = useStyles();
  return (
    <>
      <Text style={styles.kicker}>Needs approval</Text>
      <Text style={styles.code} selectable>
        {request.label}
      </Text>
      <View style={styles.actions}>
        <Button label="Deny" disabled={sending} onPress={() => respond({ requestId: request.requestId, decision: "deny" })} />
        <Button label="Allow" primary disabled={sending} onPress={() => respond({ requestId: request.requestId, decision: "allow" })} />
      </View>
    </>
  );
}

function Questions({
  request,
  sending,
  respond,
}: {
  request: Extract<SessionRequest, { kind: "question" }>;
  sending: boolean;
  respond: Respond;
}) {
  const styles = useStyles();
  const colors = useColors();
  const scheme = useScheme();
  const [selected, setSelected] = useState<Record<string, string[]>>({});
  const [typed, setTyped] = useState<Record<string, string>>({});

  const answersFor = (question: RequestQuestion) => {
    const picked = selected[question.id] ?? [];
    if (!typed[question.id]?.trim()) return picked;
    const customId = question.options.find(isOther)?.id ?? CUSTOM_OPTION_ID;
    return question.multiSelect ? [...picked, customId] : [customId];
  };
  const isComplete = (question: RequestQuestion) => {
    const answers = answersFor(question);
    return question.multiSelect ? answers.length > 0 : answers.length === 1;
  };
  // One question at a time, as the desktop's form: a skipped one is left
  // unanswered, and the last step sends whatever was answered.
  const [step, setStep] = useState(0);
  const total = request.questions.length;
  const index = Math.min(step, Math.max(total - 1, 0));
  const question = request.questions[index];
  const last = index >= total - 1;
  const finish = (skipped?: string) => {
    const answered = request.questions.filter(
      (item) => item.id !== skipped && isComplete(item),
    );
    if (answered.length === 0) return respond({ requestId: request.requestId, skip: true });
    respond({
      requestId: request.requestId,
      answers: Object.fromEntries(answered.map((item) => [item.id, answersFor(item)])),
      custom: Object.fromEntries(
        answered.flatMap((item) =>
          typed[item.id]?.trim() ? [[item.id, typed[item.id].trim()]] : [],
        ),
      ),
    });
  };
  const skip = () => {
    if (!question) return finish();
    setSelected((previous) => ({ ...previous, [question.id]: [] }));
    setTyped((previous) => ({ ...previous, [question.id]: "" }));
    if (last) finish(question.id);
    else setStep(index + 1);
  };
  const toggle = (question: RequestQuestion, id: string) => {
    const picked = selected[question.id] ?? [];
    setSelected((previous) => ({
      ...previous,
      [question.id]: question.multiSelect
        ? picked.includes(id)
          ? picked.filter((item) => item !== id)
          : [...picked, id]
        : [id],
    }));
    // One answer to a single choice: picking an option drops the typed one.
    if (!question.multiSelect) setTyped((previous) => ({ ...previous, [question.id]: "" }));
  };

  if (!question) return null;
  const picked = selected[question.id] ?? [];
  const canType = question.allowCustom || question.options.some(isOther);
  return (
    <>
      <View style={styles.header}>
        <Text style={[styles.kicker, styles.grow]} numberOfLines={1}>
          {question.header?.trim() || request.title?.trim() || "Question"}
        </Text>
        {total > 1 ? <Text style={styles.count}>{index + 1} of {total}</Text> : null}
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={last ? "Skip and send" : "Skip this question"}
          disabled={sending}
          hitSlop={8}
          onPress={skip}
        >
          <Text style={styles.link}>Skip</Text>
        </Pressable>
      </View>
      <View key={question.id} style={styles.question}>
        <Text style={styles.prompt}>{question.prompt}</Text>
        {question.options.filter((option) => !isOther(option)).map((option) => {
          const on = picked.includes(option.id);
          return (
            <Pressable
              key={option.id}
              accessibilityRole={question.multiSelect ? "checkbox" : "radio"}
              accessibilityState={{ checked: on, disabled: sending }}
              disabled={sending}
              onPress={() => toggle(question, option.id)}
              style={({ pressed }) => [styles.option, on && styles.optionOn, pressed && styles.pressed]}
            >
              <Text style={[styles.optionLabel, on && styles.onLabel]}>{option.label}</Text>
              {option.description ? <Text style={[styles.optionHint, on && styles.onHint]}>{option.description}</Text> : null}
            </Pressable>
          );
        })}
        {canType ? (
          <TextInput
            value={typed[question.id] ?? ""}
            editable={!sending}
            onChangeText={(text) => {
              setTyped((previous) => ({ ...previous, [question.id]: text }));
              if (!question.multiSelect && text.trim())
                setSelected((previous) => ({ ...previous, [question.id]: [] }));
            }}
            placeholder={question.options.length ? "Or type your answer" : "Type your answer"}
            placeholderTextColor={colors.muted}
            keyboardAppearance={scheme}
            multiline
            style={styles.input}
          />
        ) : null}
      </View>
      <View style={styles.actions}>
        {index > 0 ? (
          <Button label="Back" disabled={sending} onPress={() => setStep(index - 1)} />
        ) : null}
        <Button
          label={last ? "Submit" : "Continue"}
          primary
          disabled={sending || !isComplete(question)}
          onPress={() => (last ? finish() : setStep(index + 1))}
        />
      </View>
    </>
  );
}

function Button({
  label,
  onPress,
  primary,
  disabled,
}: {
  label: string;
  onPress: () => void;
  primary?: boolean;
  disabled?: boolean;
}) {
  const styles = useStyles();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityState={{ disabled }}
      disabled={disabled}
      onPress={onPress}
      style={({ pressed }) => [
        styles.button,
        primary && styles.primary,
        disabled && styles.disabled,
        pressed && styles.pressed,
      ]}
    >
      <Text style={[styles.buttonLabel, primary && styles.primaryLabel]}>{label}</Text>
    </Pressable>
  );
}

const useStyles = themedStyles((colors) => ({
  // Over the composer, at its expanded margins.
  card: {
    marginHorizontal: 12,
    marginBottom: 8,
    borderRadius: RADIUS,
    overflow: "hidden",
  },
  content: {
    padding: 14,
    gap: 10,
  },
  kicker: {
    color: colors.caution,
    fontSize: 12,
    fontWeight: "600",
    textTransform: "uppercase",
  },
  code: {
    color: colors.content,
    fontSize: 14,
    fontFamily: "monospace",
    padding: 10,
    borderRadius: 10,
    backgroundColor: colors.fill,
  },
  question: {
    gap: 6,
  },
  header: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
  },
  grow: {
    flex: 1,
  },
  count: {
    color: colors.muted,
    fontSize: 12,
  },
  link: {
    color: colors.muted,
    fontSize: 13,
    fontWeight: "600",
  },
  prompt: {
    color: colors.content,
    fontSize: 15,
    fontWeight: "500",
  },
  // The composer's own surfaces: fill tints without borders, its 36pt pills,
  // and the send button's ink for what is chosen.
  option: {
    paddingHorizontal: 14,
    paddingVertical: 9,
    borderRadius: 18,
    backgroundColor: colors.fill,
    gap: 2,
  },
  optionOn: {
    backgroundColor: colors.inverseSurface,
  },
  optionLabel: {
    color: colors.content,
    fontSize: 15,
    fontWeight: "500",
  },
  optionHint: {
    color: colors.muted,
    fontSize: 13,
  },
  onLabel: {
    color: colors.inverse,
  },
  onHint: {
    color: colors.inverse,
    opacity: 0.7,
  },
  input: {
    minHeight: 36,
    paddingHorizontal: 14,
    paddingVertical: 8,
    borderRadius: 18,
    backgroundColor: colors.fill,
    color: colors.content,
    fontSize: 15,
  },
  actions: {
    flexDirection: "row",
    gap: 8,
  },
  button: {
    flex: 1,
    height: 36,
    borderRadius: 18,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.fill,
  },
  primary: {
    backgroundColor: colors.inverseSurface,
  },
  buttonLabel: {
    color: colors.content,
    fontSize: 15,
    fontWeight: "500",
  },
  primaryLabel: {
    color: colors.inverse,
  },
  disabled: {
    opacity: 0.4,
  },
  pressed: {
    opacity: 0.6,
  },
}));

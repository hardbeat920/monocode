import { useRef, useState } from "react";
import { Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import type {
  ApprovalDecision,
  ApprovalPrompt,
} from "@/features/workspace/types";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { LiquidView } from "@/ui/LiquidView";
import { radii, themedStyles } from "@/ui/theme";

export function ApprovalCard({
  conversationId,
  prompts,
}: {
  conversationId: string;
  prompts: ApprovalPrompt[];
}) {
  const [selected, setSelected] = useState<string | null>(null);
  const styles = useStyles();
  const found = prompts.findIndex((prompt) => prompt.token === selected);
  const index = Math.max(0, found);
  const prompt = prompts[index];
  return prompt ? (
    <View>
      {prompts.length > 1 ? (
        <View style={styles.paging}>
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Previous approval"
            disabled={index === 0}
            onPress={() => setSelected(prompts[index - 1].token)}
            style={styles.button}
          >
            <Text style={styles.label}>Previous</Text>
          </Pressable>
          <Text style={styles.meta}>
            {index + 1} of {prompts.length} approvals
          </Text>
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Next approval"
            disabled={index === prompts.length - 1}
            onPress={() => setSelected(prompts[index + 1].token)}
            style={styles.button}
          >
            <Text style={styles.label}>Next</Text>
          </Pressable>
        </View>
      ) : null}
      <ApprovalRequest
        key={`${conversationId}:${prompt.token}`}
        conversationId={conversationId}
        prompt={prompt}
        count={prompts.length}
      />
    </View>
  ) : null;
}
function ApprovalRequest({
  conversationId,
  prompt,
  count,
}: {
  conversationId: string;
  prompt: ApprovalPrompt;
  count: number;
}) {
  const styles = useStyles();
  const { respondApproval } = useWorkspace();
  const flight = useRef(false);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const send = async (decision: ApprovalDecision) => {
    if (flight.current) return;
    flight.current = true;
    setSending(true);
    setError(null);
    try {
      await respondApproval(conversationId, prompt, decision);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      flight.current = false;
      setSending(false);
    }
  };
  return (
    <View style={styles.card}>
      <LiquidView
        fill
        row={false}
        borderRadius={radii.card}
        style={StyleSheet.absoluteFill}
      >
        {null}
      </LiquidView>
      <Text style={styles.meta}>
        Approval required{count > 1 ? ` · ${count} pending` : ""}
      </Text>
      <ScrollView style={styles.body} nestedScrollEnabled>
        <Text style={styles.title}>{prompt.title}</Text>
        {prompt.detail ? (
          <Text selectable style={styles.detail}>
            {prompt.detail}
          </Text>
        ) : null}
      </ScrollView>
      {prompt.unsupported ? (
        <Text style={styles.meta}>{prompt.unsupported}</Text>
      ) : null}
      {error ? (
        <Text accessibilityRole="alert" style={styles.error}>
          Decision not sent: {error}
        </Text>
      ) : null}
      <View style={styles.actions}>
        {prompt.decisions.map((decision) => (
          <Pressable
            key={decision.id}
            accessibilityRole="button"
            accessibilityState={{ disabled: sending }}
            disabled={sending}
            onPress={() => void send(decision.id)}
            style={({ pressed }) => [
              styles.button,
              (pressed || sending) && styles.disabled,
            ]}
          >
            <Text style={styles.label}>{decision.label}</Text>
          </Pressable>
        ))}
      </View>
      {sending ? <Text style={styles.meta}>Sending decision…</Text> : null}
    </View>
  );
}
const useStyles = themedStyles((colors) => ({
  card: {
    marginHorizontal: 12,
    marginBottom: 8,
    padding: 16,
    gap: 12,
    borderRadius: radii.card,
    overflow: "hidden",
  },
  body: { maxHeight: 240 },
  title: {
    color: colors.content,
    fontSize: 16,
    fontWeight: "600",
    marginBottom: 8,
  },
  detail: {
    color: colors.content,
    fontSize: 12,
    fontFamily: "monospace",
    lineHeight: 18,
  },
  meta: { color: colors.muted, fontSize: 12 },
  error: { color: colors.content, fontSize: 13 },
  paging: {
    marginHorizontal: 12,
    marginBottom: 8,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
  },
  actions: { flexDirection: "row", flexWrap: "wrap", gap: 8 },
  button: {
    paddingHorizontal: 14,
    paddingVertical: 12,
    borderRadius: 12,
    backgroundColor: colors.surface,
  },
  label: { color: colors.content, fontSize: 14, fontWeight: "600" },
  disabled: { opacity: 0.45 },
}));

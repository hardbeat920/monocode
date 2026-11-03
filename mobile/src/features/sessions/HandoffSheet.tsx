import { useSafeAreaInsets } from "react-native-safe-area-context";
import { createContext, use, useRef, useState, type ReactNode } from "react";
import {
  ActivityIndicator,
  Keyboard,
  Modal,
  Platform,
  Pressable,
  ScrollView,
  Text,
  View,
} from "react-native";

import { effortSetting, settingsFor } from "@/features/models/modelChoice";
import type { CatalogModel, ModelCatalog } from "@/features/models/types";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import type { Harness } from "@/features/workspace/types";
import { Icon } from "@/ui/Icon";
import { themedStyles, useColors } from "@/ui/theme";
import { useOpenLaunchedSession } from "./LaunchedSessionCard";
import { ProviderLogo } from "./ProviderLogo";

const Handoff = createContext<((turnId: string, from: Harness) => void) | null>(
  null,
);
export const useHandoff = () => use(Handoff);

/** One picker for both the React and native transcript footers. */
export function HandoffProvider({
  conversationId,
  projectId,
  children,
}: {
  conversationId: string;
  projectId: string;
  children: ReactNode;
}) {
  const colors = useColors();
  const insets = useSafeAreaInsets();
  const styles = useStyles();
  const { listModels, handoffSession } = useWorkspace();
  const openSession = useOpenLaunchedSession();
  const [request, setRequest] = useState<{
    turnId: string;
    from: Harness;
  } | null>(null);
  const [catalog, setCatalog] = useState<ModelCatalog | null>(null);
  const [model, setModel] = useState<CatalogModel | null>(null);
  const [effort, setEffort] = useState<string>();
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const pending = useRef(false);
  const destination = useRef<string | null>(null);
  const generation = useRef(0);
  const dismiss = () => {
    if (pending.current) return;
    generation.current += 1;
    setRequest(null);
  };
  const show = (turnId: string, from: Harness) => {
    Keyboard.dismiss();
    const token = ++generation.current;
    setRequest({ turnId, from });
    setCatalog(null);
    setModel(null);
    setError(null);
    listModels(projectId, conversationId).then(
      (next) => {
        if (generation.current === token) setCatalog(next);
      },
      (reason) => {
        if (generation.current === token)
          setError(String(reason instanceof Error ? reason.message : reason));
      },
    );
  };
  const setting = effortSetting(model ?? undefined);
  const submit = async () => {
    if (!request || !model || pending.current) return;
    pending.current = true;
    setSending(true);
    setError(null);
    try {
      const result = await handoffSession(conversationId, request.turnId, {
        harness: model.harness,
        model: model.id,
        modelSettings: {
          ...settingsFor(model),
          ...(setting && effort ? { [setting.id]: effort } : {}),
        },
      });
      setRequest(null);
      // iOS must finish dismissing this sheet before presenting the session.
      if (Platform.OS === "ios") destination.current = result.sessionId;
      else openSession(projectId, result.sessionId);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      pending.current = false;
      setSending(false);
    }
  };
  const targets =
    catalog?.harnesses.filter((provider) => provider.id !== request?.from) ??
    [];
  return (
    <Handoff value={show}>
      {children}
      <Modal
        visible={!!request}
        animationType="slide"
        presentationStyle="pageSheet"
        onRequestClose={dismiss}
        onDismiss={() => {
          const id = destination.current;
          destination.current = null;
          if (id) openSession(projectId, id);
        }}
      >
        <View
          style={[
            styles.sheet,
            {
              paddingBottom: Math.max(24, insets.bottom),
              paddingTop: Math.max(32, insets.top),
            },
          ]}
        >
          <View style={styles.header}>
            <Text accessibilityRole="header" style={styles.title}>
              Handoff
            </Text>
            <Pressable
              onPress={dismiss}
              disabled={sending}
              accessibilityRole="button"
              accessibilityLabel="Close handoff"
              hitSlop={12}
            >
              <Icon name="close" size={24} />
            </Pressable>
          </View>
          <Text style={styles.note}>
            Continue in a new session with another provider. The conversation
            through this reply is included.
          </Text>
          <ScrollView contentContainerStyle={styles.list}>
            {!catalog && !error ? (
              <ActivityIndicator color={colors.muted} />
            ) : null}
            {targets.map((provider) => (
              <View key={provider.id} style={styles.group}>
                <View style={styles.provider}>
                  <ProviderLogo harness={provider.id} size={18} />
                  <Text style={styles.label}>{provider.label}</Text>
                </View>
                {catalog?.models
                  .filter((entry) => entry.harness === provider.id)
                  .map((entry) => (
                    <Pressable
                      key={entry.id}
                      disabled={sending}
                      accessibilityRole="button"
                      accessibilityState={{ selected: model === entry }}
                      onPress={() => {
                        setModel(entry);
                        setEffort(effortSetting(entry)?.value);
                      }}
                      style={styles.row}
                    >
                      <Text style={styles.text}>{entry.name}</Text>
                      {model === entry ? (
                        <Icon name="check" size={18} color={colors.accent} />
                      ) : null}
                    </Pressable>
                  ))}
              </View>
            ))}
            {catalog &&
            !targets.some((provider) =>
              catalog.models.some((entry) => entry.harness === provider.id),
            ) ? (
              <Text style={styles.note}>
                Install another provider on your desktop to hand off.
              </Text>
            ) : null}
            {setting ? (
              <View style={styles.group}>
                <Text style={styles.label}>{setting.label}</Text>
                {setting.options.map((option) => (
                  <Pressable
                    key={option.value}
                    disabled={sending}
                    accessibilityRole="button"
                    accessibilityState={{ selected: effort === option.value }}
                    onPress={() => setEffort(option.value)}
                    style={styles.row}
                  >
                    <Text style={styles.text}>{option.label}</Text>
                    {effort === option.value ? (
                      <Icon name="check" size={18} color={colors.accent} />
                    ) : null}
                  </Pressable>
                ))}
              </View>
            ) : null}
            {error ? (
              <Text accessibilityRole="alert" style={styles.note}>
                {error}
              </Text>
            ) : null}
          </ScrollView>
          <Pressable
            accessibilityRole="button"
            disabled={!model || sending}
            onPress={() => void submit()}
            style={[styles.submit, (!model || sending) && styles.disabled]}
          >
            {sending ? (
              <ActivityIndicator color={colors.background} />
            ) : (
              <Text style={styles.submitText}>Create handoff</Text>
            )}
          </Pressable>
        </View>
      </Modal>
    </Handoff>
  );
}
const useStyles = themedStyles((colors) => ({
  sheet: {
    flex: 1,
    backgroundColor: colors.sheet,
    padding: 24,
    paddingTop: 32,
    paddingBottom: 40,
    gap: 20,
  },
  header: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
  },
  title: { color: colors.content, fontSize: 24, fontWeight: "600" },
  note: { color: colors.muted, fontSize: 15, lineHeight: 22 },
  list: { gap: 24, paddingBottom: 16 },
  group: { gap: 8 },
  provider: { flexDirection: "row", gap: 8, alignItems: "center" },
  label: { color: colors.content, fontSize: 16, fontWeight: "600" },
  text: { color: colors.content, fontSize: 16, flex: 1 },
  row: {
    flexDirection: "row",
    alignItems: "center",
    minHeight: 44,
    gap: 12,
    paddingHorizontal: 12,
  },
  submit: {
    backgroundColor: colors.content,
    borderRadius: 16,
    padding: 16,
    alignItems: "center",
  },
  submitText: { color: colors.background, fontSize: 16, fontWeight: "600" },
  disabled: { opacity: 0.4 },
}));

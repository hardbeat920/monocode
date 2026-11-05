import * as Haptics from "expo-haptics";
import { router } from "expo-router";
import { createContext, use, useState } from "react";
import { Alert, Pressable, Text, View } from "react-native";

import type { TurnItem } from "@/features/workspace/types";
import { Icon } from "@/ui/Icon";
import { themedStyles, useColors, withAlpha } from "@/ui/theme";

type DocumentItem = Extract<TurnItem, { type: "document" }>;

/** Sends a suggested reply as the user's message; set by a Mono's chat. */
export const MonoChoose = createContext<((text: string) => Promise<void>) | null>(null);

/** What a long-pressed message in a Mono's chat can do beyond copying. */
export const MonoMessageActions = createContext<{
  reply: (text: string) => void;
  markUnread: () => void;
} | null>(null);

/** Picking a suggested reply, with a failed send told rather than lost. */
export function useMonoChoose() {
  const send = use(MonoChoose);
  if (!send) return null;
  return (text: string) => {
    void Haptics.selectionAsync();
    send(text).catch((reason: unknown) =>
      Alert.alert(
        "Couldn’t send",
        reason instanceof Error ? reason.message : String(reason),
      ),
    );
  };
}

/** A document a Mono wrote opens in a sheet, as a file does. */
export function openDocument(documentId: string, title?: string) {
  router.push({
    pathname: "/document",
    params: { documentId, ...(title ? { title } : {}) },
  });
}

/**
 * A Mono's suggested replies, as the desktop's: a card each with a ring. A tap
 * sends it; once a reply follows, the one sent is checked and the rest step
 * back.
 */
export function MonoChoices({ options, chosen }: { options: string[]; chosen?: string }) {
  const styles = useStyles();
  const colors = useColors();
  const choose = useMonoChoose();
  // Answered at the tap, so a second tap can't send another before the
  // reply comes back from the desktop.
  const [sent, setSent] = useState<string | null>(null);
  const reply = chosen ?? sent;
  const answered = reply != null;
  return (
    <View style={styles.list}>
      {options.map((option, index) => {
        const checked = answered && option.trim() === reply.trim();
        return (
          <Pressable
            key={index}
            accessibilityRole="button"
            accessibilityLabel={option}
            accessibilityState={{ checked, disabled: answered || !choose }}
            disabled={answered || !choose}
            onPress={() => {
              setSent(option);
              choose?.(option);
            }}
            style={({ pressed }) => [
              styles.choice,
              checked && styles.checked,
              answered && !checked && styles.dimmed,
              pressed && styles.pressed,
            ]}
          >
            {checked ? (
              <View style={styles.tick}>
                <Icon name="check" size={11} color="#062e22" />
              </View>
            ) : (
              <View style={[styles.ring, { borderColor: withAlpha(colors.content, 0.35) }]} />
            )}
            <Text style={styles.label}>{option}</Text>
          </Pressable>
        );
      })}
    </View>
  );
}

/** ArtifactCard: the document's mark and title; a tap opens the reader. */
export function DocumentCard({ document }: { document: DocumentItem }) {
  const styles = useStyles();
  const colors = useColors();
  const visual = document.kind === "html" || document.kind === "visual";
  return (
    <View style={styles.list}>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={`Open ${visual ? "visual" : "document"} ${document.title}`}
        onPress={() => openDocument(document.documentId, document.title)}
        style={({ pressed }) => [styles.document, pressed && styles.pressed]}
      >
        <Icon
          name={visual ? "metrics" : "note"}
          size={16}
          color={withAlpha(colors.content, 0.55)}
        />
        <Text style={styles.title} numberOfLines={1}>
          {document.title}
        </Text>
      </Pressable>
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  list: {
    alignItems: "flex-start",
    gap: 6,
    paddingHorizontal: 20,
    paddingTop: 4,
    paddingBottom: 8,
  },
  choice: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    maxWidth: "80%",
    borderRadius: 12,
    borderWidth: 1,
    borderColor: withAlpha(colors.content, 0.1),
    paddingVertical: 6,
    paddingLeft: 10,
    paddingRight: 12,
  },
  checked: {
    borderColor: withAlpha(colors.content, 0.2),
  },
  dimmed: {
    opacity: 0.45,
  },
  pressed: {
    backgroundColor: withAlpha(colors.content, 0.05),
  },
  ring: {
    width: 16,
    height: 16,
    borderRadius: 8,
    borderWidth: 1.5,
  },
  tick: {
    width: 16,
    height: 16,
    borderRadius: 8,
    backgroundColor: "#34d399",
    alignItems: "center",
    justifyContent: "center",
  },
  label: {
    flexShrink: 1,
    color: colors.content,
    fontSize: 14,
    lineHeight: 20,
  },
  document: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    maxWidth: "100%",
    minHeight: 38,
    borderRadius: 12,
    borderWidth: 1,
    borderColor: colors.stroke,
    backgroundColor: withAlpha(colors.content, 0.03),
    paddingLeft: 12,
    paddingRight: 14,
    paddingVertical: 8,
  },
  title: {
    flexShrink: 1,
    color: colors.content,
    fontSize: 14,
    lineHeight: 20,
  },
}));

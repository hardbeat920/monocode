import * as Haptics from "expo-haptics";
import { router, Stack } from "expo-router";
import { useState } from "react";
import { ActivityIndicator, Alert, Pressable, Text, TextInput, View } from "react-native";
import { KeyboardAvoidingView } from "react-native-keyboard-controller";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { HeaderButton } from "@/features/files/codeScreen";
import { themedStyles, useColors, useScheme } from "@/ui/theme";

import { reloadInboxDetail } from "./detail";
import { useInbox } from "./InboxProvider";

/** Unsent text by item and thread, so swiping the sheet away loses nothing. */
const unsent = new Map<string, string>();

/**
 * Writes a comment on an item, or a reply when `replyTo` is set. The desktop
 * posts it as the user's own account at the provider.
 */
export function CommentSheet({
  itemKey,
  replyTo,
  author,
}: {
  itemKey: string;
  replyTo?: string;
  author?: string;
}) {
  const colors = useColors();
  const scheme = useScheme();
  const styles = useStyles();
  const { bottom } = useSafeAreaInsets();
  const { client, notify } = useInbox();
  const place = `${itemKey}\n${replyTo ?? ""}`;
  const [text, setText] = useState(() => unsent.get(place) ?? "");
  const [posting, setPosting] = useState(false);
  const ready = text.trim().length > 0 && !posting;

  const change = (next: string) => {
    setText(next);
    unsent.set(place, next);
  };

  const post = async () => {
    setPosting(true);
    try {
      await client.comment(itemKey, text.trim(), replyTo);
      unsent.delete(place);
      void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
      reloadInboxDetail(itemKey);
      notify(replyTo ? "Reply posted" : "Comment posted");
      router.back();
    } catch (reason) {
      setPosting(false);
      Alert.alert(
        "Couldn’t post",
        reason instanceof Error ? reason.message : String(reason),
      );
    }
  };

  return (
    <KeyboardAvoidingView behavior="padding" style={styles.root}>
      <Stack.Screen
        options={{
          title: replyTo ? `Reply to ${author || "comment"}` : "Add comment",
          headerLeft: () => (
            <HeaderButton icon="close" label="Close" onPress={() => router.back()} />
          ),
        }}
      />
      <TextInput
        value={text}
        onChangeText={change}
        multiline
        autoFocus
        editable={!posting}
        placeholder={replyTo ? "Write a reply" : "Write a comment"}
        placeholderTextColor={colors.muted}
        keyboardAppearance={scheme}
        selectionColor={colors.accent}
        textAlignVertical="top"
        style={styles.input}
      />
      <View style={[styles.bar, { paddingBottom: bottom + 12 }]}>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={replyTo ? "Post reply" : "Post comment"}
          accessibilityState={{ disabled: !ready, busy: posting }}
          disabled={!ready}
          onPress={() => void post()}
          style={({ pressed }) => [
            styles.primary,
            !ready && styles.disabled,
            pressed && styles.pressed,
          ]}
        >
          {posting ? (
            <ActivityIndicator color={colors.inverse} />
          ) : (
            <Text style={styles.primaryLabel}>{replyTo ? "Post reply" : "Post comment"}</Text>
          )}
        </Pressable>
      </View>
    </KeyboardAvoidingView>
  );
}

const useStyles = themedStyles((colors) => ({
  root: {
    flex: 1,
  },
  input: {
    flex: 1,
    paddingTop: 16,
    paddingHorizontal: 16,
    color: colors.content,
    fontSize: 17,
    lineHeight: 23,
  },
  bar: {
    paddingTop: 12,
    paddingHorizontal: 16,
  },
  primary: {
    height: 50,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: 25,
    backgroundColor: colors.inverseSurface,
  },
  primaryLabel: {
    color: colors.inverse,
    fontSize: 17,
    fontWeight: "600",
  },
  disabled: {
    opacity: 0.35,
  },
  pressed: {
    opacity: 0.7,
  },
}));

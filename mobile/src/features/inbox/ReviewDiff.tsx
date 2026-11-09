import { useCallback, useState } from "react";
import {
  ActivityIndicator,
  Alert,
  KeyboardAvoidingView,
  Modal,
  Platform,
  Pressable,
  Text,
  TextInput,
  View,
} from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { MONO } from "@/features/changes/diffTheme";
import { DiffScreen } from "@/features/changes/DiffScreen";
import type { ChangedFile, DiffLine } from "@/features/workspace/types";
import { themedStyles, useColors, useScheme } from "@/ui/theme";

import { useInbox } from "./InboxProvider";
import type { InboxReviewLine } from "./protocol";
import { countComments, lineLabel, reviewLine, sameLine, useReviewComments } from "./review";

type Target = InboxReviewLine & { code: string };

/**
 * A pull request's file whose lines take review comments. Tapping a line writes
 * one, which waits with the others, here and on the desktop, until the review
 * is posted from the Code tab. A line with one shows a dot.
 */
export function ReviewDiff({ file, itemKey }: { file: ChangedFile; itemKey: string }) {
  const styles = useStyles();
  const { bottom } = useSafeAreaInsets();
  const { review } = useInbox();
  const comments = useReviewComments(review, itemKey);
  const [target, setTarget] = useState<Target | null>(null);
  const path = file.relative;

  const onLine = useCallback(
    (line: DiffLine) => {
      const place = reviewLine(path, line);
      if (place) setTarget({ ...place, code: line.text });
    },
    [path],
  );
  const isMarked = useCallback(
    (line: DiffLine) => {
      const place = reviewLine(path, line);
      return !!place && comments.some((comment) => sameLine(comment, place));
    },
    [path, comments],
  );

  return (
    <View style={styles.root}>
      <DiffScreen file={file} onLine={onLine} isMarked={isMarked} />
      <View pointerEvents="none" style={[styles.hint, { bottom: bottom + 12 }]}>
        <Text style={styles.hintLabel}>
          {comments.length > 0
            ? `${countComments(comments.length)} pending · post from the Code tab`
            : "Tap a line to comment"}
        </Text>
      </View>
      {target ? (
        <LineComposer itemKey={itemKey} target={target} onClose={() => setTarget(null)} />
      ) : null}
    </View>
  );
}

/** Writes one line's comment, over the diff so the line stays in view behind it. */
function LineComposer({
  itemKey,
  target,
  onClose,
}: {
  itemKey: string;
  target: Target;
  onClose: () => void;
}) {
  const colors = useColors();
  const scheme = useScheme();
  const styles = useStyles();
  const { bottom } = useSafeAreaInsets();
  const { review } = useInbox();
  const [text, setText] = useState("");
  const [adding, setAdding] = useState(false);
  const ready = text.trim().length > 0 && !adding;

  const add = async () => {
    setAdding(true);
    try {
      const { path, line, side } = target;
      await review.add(itemKey, { path, line, side }, text.trim());
      onClose();
    } catch (reason) {
      setAdding(false);
      Alert.alert(
        "Couldn’t add the comment",
        reason instanceof Error ? reason.message : String(reason),
      );
    }
  };

  return (
    <Modal visible transparent animationType="slide" onRequestClose={onClose}>
      <KeyboardAvoidingView
        behavior={Platform.OS === "ios" ? "padding" : undefined}
        style={styles.overlay}
      >
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Cancel"
          onPress={onClose}
          style={styles.backdrop}
        />
        <View style={[styles.card, { paddingBottom: bottom + 12 }]}>
          <Text style={styles.place}>{lineLabel(target)}</Text>
          <Text numberOfLines={1} style={styles.code}>
            {target.code.trim() || " "}
          </Text>
          <TextInput
            value={text}
            onChangeText={setText}
            multiline
            autoFocus
            editable={!adding}
            placeholder="Comment on this line"
            placeholderTextColor={colors.muted}
            keyboardAppearance={scheme}
            selectionColor={colors.accent}
            textAlignVertical="top"
            style={styles.input}
          />
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Add to review"
            accessibilityState={{ disabled: !ready, busy: adding }}
            disabled={!ready}
            onPress={() => void add()}
            style={({ pressed }) => [
              styles.add,
              !ready && styles.disabled,
              pressed && styles.pressed,
            ]}
          >
            {adding ? (
              <ActivityIndicator color={colors.inverse} />
            ) : (
              <Text style={styles.addLabel}>Add to review</Text>
            )}
          </Pressable>
        </View>
      </KeyboardAvoidingView>
    </Modal>
  );
}

const useStyles = themedStyles((colors) => ({
  root: {
    flex: 1,
  },
  hint: {
    position: "absolute",
    alignSelf: "center",
    paddingVertical: 8,
    paddingHorizontal: 14,
    borderRadius: 18,
    backgroundColor: colors.inverseSurface,
  },
  hintLabel: {
    color: colors.inverse,
    fontSize: 13,
    fontWeight: "600",
  },
  overlay: {
    flex: 1,
    justifyContent: "flex-end",
  },
  backdrop: {
    flex: 1,
  },
  card: {
    gap: 8,
    paddingTop: 16,
    paddingHorizontal: 16,
    borderTopLeftRadius: 20,
    borderTopRightRadius: 20,
    borderCurve: "continuous",
    backgroundColor: colors.sheet,
    borderWidth: 1,
    borderColor: colors.stroke,
  },
  place: {
    color: colors.content,
    fontSize: 15,
    fontWeight: "600",
  },
  code: {
    color: colors.muted,
    fontFamily: MONO,
    fontSize: 12,
  },
  input: {
    minHeight: 96,
    maxHeight: 200,
    color: colors.content,
    fontSize: 17,
    lineHeight: 23,
  },
  add: {
    height: 50,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: 25,
    backgroundColor: colors.inverseSurface,
  },
  addLabel: {
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

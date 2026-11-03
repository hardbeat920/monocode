import { useHeaderHeight } from "expo-router/react-navigation";
import { router, Stack, useLocalSearchParams } from "expo-router";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  ActivityIndicator,
  Alert,
  Platform,
  Pressable,
  Text,
  TextInput,
} from "react-native";
import { KeyboardAvoidingView } from "react-native-keyboard-controller";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { useConnection } from "@/features/connection/ConnectionProvider";
import { useOffline } from "@/features/connection/ConnectionNotice";
import { HeaderButton } from "@/features/files/codeScreen";
import { themedStyles, useColors, useScheme } from "@/ui/theme";

export type MonoFile = "soul" | "memory";

const TITLES: Record<MonoFile, string> = { soul: "Soul", memory: "Memory" };
const PLACEHOLDERS: Record<MonoFile, string> = {
  soul: "Who it is, and what it should always keep in mind",
  memory: "What it remembers between conversations",
};

// A Mono's soul or memory as Markdown, as the desktop's editor has them. It
// saves over the text it opened, so a change the Mono made meanwhile is never
// lost; Save keeps it, and leaving with changes saves them too.
export default function MonoFileScreen() {
  const { monoId, file } = useLocalSearchParams<{
    monoId: string;
    file: MonoFile;
  }>();
  const { command } = useConnection();
  const offline = useOffline();
  const styles = useStyles();
  const colors = useColors();
  const scheme = useScheme();
  const headerHeight = useHeaderHeight();
  const { bottom } = useSafeAreaInsets();
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // What was opened or last saved, and its hash: a save must match it on
  // the desktop. Mirrored in refs for the save on leaving.
  const input = useRef<TextInput>(null);
  const [base, setBase] = useState<{ text: string; hash: string } | null>(null);
  const opened = useRef(base);
  const latest = useRef(text);
  useEffect(() => {
    opened.current = base;
    latest.current = text;
  }, [base, text]);
  const title = TITLES[file] ?? "File";

  useEffect(() => {
    command<{ text: string; hash: string }>("mono.file", { monoId, file })
      .then((loaded) => {
        setBase(loaded);
        setText(loaded.text);
      })
      .catch((reason: unknown) =>
        setError(reason instanceof Error ? reason.message : String(reason)),
      );
  }, [command, monoId, file]);

  const save = useCallback(async () => {
    const from = opened.current;
    const next = latest.current;
    if (!from || next === null || next === from.text) return;
    const { hash } = await command<{ hash: string }>("mono.saveFile", {
      monoId,
      file,
      text: next,
      expectedHash: from.hash,
    });
    opened.current = { text: next, hash };
    setBase(opened.current);
  }, [command, monoId, file]);

  // Leaving with changes, by back or by closing the sheet, saves them.
  useEffect(
    () => () => {
      save().catch((reason: unknown) =>
        Alert.alert(
          `Couldn’t save ${title.toLowerCase()}`,
          reason instanceof Error ? reason.message : String(reason),
        ),
      );
    },
    [save, title],
  );

  const dirty = text !== null && base !== null && text !== base.text;
  return (
    <KeyboardAvoidingView behavior="padding" style={styles.screen}>
      <Stack.Screen
        options={{
          title,
          headerRight:
            dirty && !offline
              ? () => (
                  <HeaderButton
                    icon="check"
                    label="Save"
                    onPress={() =>
                      save()
                        .then(() => router.back())
                        .catch((reason: unknown) =>
                          Alert.alert(
                            "Couldn’t save",
                            reason instanceof Error
                              ? reason.message
                              : String(reason),
                          ),
                        )
                    }
                  />
                )
              : undefined,
        }}
      />
      {text === null ? (
        error ? (
          <Text style={[styles.note, { marginTop: headerHeight + 24 }]}>
            {error}
          </Text>
        ) : (
          <ActivityIndicator
            color={colors.muted}
            style={{ marginTop: headerHeight + 32 }}
          />
        )
      ) : (
        // Below the header rather than under it, so iOS adds no inset of its
        // own; a tap anywhere below the text still lands in the editor.
        <Pressable
          accessible={false}
          onPress={() => input.current?.focus()}
          style={[styles.editor, { marginTop: headerHeight }]}
        >
          <TextInput
            ref={input}
            value={text}
            onChangeText={setText}
            multiline
            autoCorrect={false}
            autoCapitalize="sentences"
            textAlignVertical="top"
            placeholder={PLACEHOLDERS[file]}
            placeholderTextColor={colors.muted}
            keyboardAppearance={scheme}
            editable={!offline}
            style={[
              styles.input,
              { paddingBottom: bottom + 24 },
              offline && styles.off,
            ]}
          />
        </Pressable>
      )}
    </KeyboardAvoidingView>
  );
}

const useStyles = themedStyles((colors) => ({
  screen: {
    flex: 1,
  },
  editor: {
    flex: 1,
  },
  input: {
    flex: 1,
    paddingTop: 12,
    color: colors.content,
    fontFamily: Platform.select({ ios: "Menlo", default: "monospace" }),
    fontSize: 14,
    lineHeight: 21,
    paddingHorizontal: 20,
  },
  off: {
    opacity: 0.4,
  },
  note: {
    color: colors.muted,
    fontSize: 14,
    textAlign: "center",
    marginHorizontal: 24,
  },
}));

import { BottomSheet, RNHostView } from "@expo/ui";
import { useEffect, useRef, useState } from "react";
import { Pressable, Text, TextInput, useWindowDimensions, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { themedStyles, useColors, useScheme } from "@/ui/theme";

/** Takes a pairing link copied from the desktop, the keyboard up as it opens. */
export function PairingLinkSheet({
  isPresented,
  onDismiss,
  onConnect,
}: {
  isPresented: boolean;
  onDismiss: () => void;
  onConnect: (link: string) => void;
}) {
  const colors = useColors();
  const scheme = useScheme();
  const styles = useStyles();
  const { width } = useWindowDimensions();
  const { bottom } = useSafeAreaInsets();
  const input = useRef<TextInput>(null);
  const [link, setLink] = useState("");
  // Focused once the sheet is up, as focusing during its slide drops the keyboard.
  useEffect(() => {
    if (!isPresented) return;
    const timer = setTimeout(() => input.current?.focus(), 350);
    return () => clearTimeout(timer);
  }, [isPresented]);
  const dismiss = () => {
    setLink("");
    onDismiss();
  };
  const connect = () => {
    const value = link.trim();
    if (!value) return;
    setLink("");
    onConnect(value);
  };
  return (
    <BottomSheet
      isPresented={isPresented}
      onDismiss={dismiss}
      contentPadding={0}
      containerColor={colors.sheet}
    >
      <RNHostView matchContents={{ vertical: true }}>
        <View style={[styles.sheet, { width, paddingBottom: Math.max(bottom, 16) + 8 }]}>
          <Text accessibilityRole="header" style={styles.title}>
            Paste a pairing link
          </Text>
          <Text style={styles.hint}>
            On your desktop, open Settings → Connections, choose Pair phone, and
            copy the link.
          </Text>
          <TextInput
            ref={input}
            accessibilityLabel="Pairing link"
            placeholder="monocode://pair…"
            placeholderTextColor={colors.muted}
            value={link}
            onChangeText={setLink}
            onSubmitEditing={connect}
            autoCapitalize="none"
            autoCorrect={false}
            // The link carries the pairing secret.
            secureTextEntry
            returnKeyType="go"
            keyboardAppearance={scheme}
            cursorColor={colors.accent}
            selectionColor={colors.accent}
            style={styles.input}
          />
          <Pressable
            accessibilityRole="button"
            disabled={!link.trim()}
            onPress={connect}
            style={({ pressed }) => [
              styles.button,
              !link.trim() && styles.dimmed,
              pressed && styles.pressed,
            ]}
          >
            <Text style={styles.buttonLabel}>Connect</Text>
          </Pressable>
        </View>
      </RNHostView>
    </BottomSheet>
  );
}

const useStyles = themedStyles((colors) => ({
  sheet: {
    paddingTop: 28,
    paddingHorizontal: 24,
    gap: 16,
  },
  title: {
    color: colors.content,
    fontSize: 20,
    fontWeight: "600",
    textAlign: "center",
  },
  hint: {
    color: colors.muted,
    fontSize: 15,
    lineHeight: 21,
    textAlign: "center",
    marginTop: -6,
  },
  input: {
    color: colors.content,
    fontSize: 16,
    backgroundColor: colors.background,
    borderRadius: 16,
    paddingHorizontal: 16,
    paddingVertical: 15,
  },
  button: {
    backgroundColor: colors.inverseSurface,
    borderRadius: 32,
    padding: 17,
    alignItems: "center",
  },
  buttonLabel: { color: colors.inverse, fontSize: 16, fontWeight: "600" },
  dimmed: { opacity: 0.4 },
  pressed: { opacity: 0.8 },
}));

import { useRef, useState, type ReactNode } from "react";
import { Pressable, TextInput } from "react-native";
import Animated from "react-native-reanimated";

import { useKeyboardLift, useSessionLayout } from "@/features/sessions/SessionScreen";
import { Icon } from "@/ui/Icon";
import { LiquidView } from "@/ui/LiquidView";
import { themedStyles, useColors, useScheme } from "@/ui/theme";

/** The pill's height: the resting composer's, so the pages' bottoms match. */
export const SEARCH_BAR = 50;

/**
 * The Files and Sessions pages' search field, where the composer rests on the chat page:
 * a glass pill that rides up with the keyboard. While it has focus a round
 * button beside it clears the search and lets go of the keyboard, as the
 * system's search field does.
 */
export function FileSearchBar({
  value,
  onChange,
  placeholder = "Search files",
  autoFocus,
  accessory,
}: {
  value: string;
  onChange: (text: string) => void;
  placeholder?: string;
  autoFocus?: boolean;
  /** A round button beside the field, such as a filter; the close button
   *  takes its place while the field has focus. */
  accessory?: ReactNode;
}) {
  const scheme = useScheme();
  const colors = useColors();
  const styles = useStyles();
  const { dockBottom, keyboardOffset } = useSessionLayout();
  const lift = useKeyboardLift(keyboardOffset);
  const input = useRef<TextInput>(null);
  const [focused, setFocused] = useState(false);

  const close = () => {
    onChange("");
    input.current?.blur();
  };

  return (
    <Animated.View style={[styles.dock, { bottom: dockBottom }, lift]}>
      {/* No onPress: it would wrap the glass in a Pressable that doesn't
          stretch across the row. The field inside takes the taps instead. */}
      <LiquidView fill style={styles.field} innerStyle={styles.fieldInner}>
        <Icon name="search" size={20} color={colors.muted} />
        <TextInput
          ref={input}
          value={value}
          onChangeText={onChange}
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
          autoFocus={autoFocus}
          placeholder={placeholder}
          placeholderTextColor={colors.muted}
          returnKeyType="search"
          autoCorrect={false}
          autoCapitalize="none"
          keyboardAppearance={scheme}
          cursorColor={colors.accent}
          selectionColor={colors.accent}
          style={styles.input}
        />
        {value ? (
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Clear search"
            onPress={() => onChange("")}
            hitSlop={10}
            style={styles.clear}
          >
            <Icon name="close" size={12} color={colors.inverse} />
          </Pressable>
        ) : null}
      </LiquidView>
      {focused ? (
        <LiquidView
          onPress={close}
          accessibilityLabel="Close search"
          borderRadius={SEARCH_BAR / 2}
          innerStyle={styles.round}
        >
          <Icon name="close" size={20} />
        </LiquidView>
      ) : (
        accessory
      )}
    </Animated.View>
  );
}

const useStyles = themedStyles((colors) => ({
  // Inset like the resting composer pill.
  dock: {
    position: "absolute",
    left: 20,
    right: 20,
    flexDirection: "row",
    gap: 10,
  },
  field: {
    flex: 1,
  },
  fieldInner: {
    height: SEARCH_BAR,
    gap: 10,
    paddingHorizontal: 18,
    justifyContent: "flex-start",
  },
  input: {
    flex: 1,
    height: SEARCH_BAR,
    color: colors.content,
    fontSize: 17,
  },
  clear: {
    width: 20,
    height: 20,
    borderRadius: 10,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.muted,
  },
  round: {
    width: SEARCH_BAR,
    height: SEARCH_BAR,
  },
}));

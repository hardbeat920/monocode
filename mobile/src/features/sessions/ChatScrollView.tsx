import { Platform, type ScrollViewProps } from "react-native";
import {
  KeyboardChatScrollView,
  type KeyboardChatScrollViewProps,
} from "react-native-keyboard-controller";

import { useSessionLayout } from "./SessionScreen";

/**
 * Scroll component for the chat list, per the keyboard-controller chat guide.
 * Keeps the latest messages above the keyboard as it opens, closes and is dragged
 * down, and as the composer grows. Lifts only when already at the end, like the
 * ChatGPT app. Pass as a FlatList `renderScrollComponent`, with `blankSpace`.
 */
export function ChatScrollView(props: ScrollViewProps & KeyboardChatScrollViewProps) {
  const { keyboardOffset, extraContentPadding } = useSessionLayout();
  return (
    <KeyboardChatScrollView
      automaticallyAdjustContentInsets={false}
      contentInsetAdjustmentBehavior="never"
      keyboardDismissMode="interactive"
      keyboardShouldPersistTaps="handled"
      keyboardLiftBehavior="whenAtEnd"
      // blankSpace is a content inset, which RN 0.81+ on iOS stops hit-testing
      // (facebook/react-native#54123).
      applyWorkaroundForContentInsetHitTestBug={Platform.OS === "ios"}
      offset={keyboardOffset}
      extraContentPadding={extraContentPadding}
      {...props}
    />
  );
}

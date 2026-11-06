import { Stack } from "expo-router";

import { modalStackOptions, sheetScreenLayout } from "@/features/files/codeScreen";
import { useColors } from "@/ui/theme";

// An inbox item and the sheets that start an agent on it or comment: a native modal over
// the project, with a stack of its own, as the model picker.
export default function InboxLayout() {
  const colors = useColors();
  return (
    <Stack
      screenOptions={modalStackOptions(colors)}
      screenLayout={sheetScreenLayout(colors.sheet)}
    >
      <Stack.Screen name="item" options={{ title: "Inbox" }} />
      <Stack.Screen name="start" options={{ title: "Start agent" }} />
      <Stack.Screen name="comment" options={{ title: "Add comment" }} />
    </Stack>
  );
}

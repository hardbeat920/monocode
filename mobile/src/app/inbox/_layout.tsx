import { Stack } from "expo-router";

import { modalStackOptions, sheetScreenLayout } from "@/features/files/codeScreen";
import { useColors } from "@/ui/theme";

// An inbox item and the sheet that starts an agent on it: a native modal over
// the project, with a stack of its own, as the model picker and agents.
export default function InboxLayout() {
  const colors = useColors();
  return (
    <Stack
      screenOptions={modalStackOptions(colors)}
      screenLayout={sheetScreenLayout(colors.sheet)}
    >
      <Stack.Screen name="item" options={{ title: "Inbox" }} />
      <Stack.Screen name="start" options={{ title: "Start agent" }} />
    </Stack>
  );
}

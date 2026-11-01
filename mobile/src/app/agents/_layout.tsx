import { router, Stack } from "expo-router";

import {
  HeaderButton,
  modalStackOptions,
  sheetScreenLayout,
} from "@/features/files/codeScreen";
import { useColors } from "@/ui/theme";

// A lead's Orchestrator agents: a native modal over its chat, as the model picker.
export default function AgentsLayout() {
  const colors = useColors();
  return (
    <Stack
      screenOptions={modalStackOptions(colors)}
      screenLayout={sheetScreenLayout(colors.sheet)}
    >
      <Stack.Screen
        name="index"
        options={{
          title: "Agents",
          headerLeft: () => (
            <HeaderButton
              icon="close"
              label="Close"
              onPress={() => router.back()}
            />
          ),
        }}
      />
    </Stack>
  );
}

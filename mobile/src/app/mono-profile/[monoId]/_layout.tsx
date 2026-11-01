import { router, Stack } from "expo-router";

import {
  HeaderButton,
  modalStackOptions,
  sheetScreenLayout,
} from "@/features/files/codeScreen";
import { androidBackHeader } from "@/ui/AndroidHeader";
import { useColors } from "@/ui/theme";

// A Mono's profile: a native modal with its own stack, as Settings, so editing
// its look pushes inside the sheet and the close button dismisses the lot.
export default function MonoProfileLayout() {
  const colors = useColors();
  return (
    <Stack
      screenOptions={modalStackOptions(colors)}
      screenLayout={sheetScreenLayout(colors.sheet)}
    >
      <Stack.Screen
        name="index"
        options={{
          title: "",
          headerLeft: () => (
            <HeaderButton icon="close" label="Close" onPress={() => router.back()} />
          ),
        }}
      />
      <Stack.Screen name="edit" options={{ title: "Edit", ...androidBackHeader("Edit") }} />
      {/* A document and a habit title themselves once read. */}
      <Stack.Screen name="document" options={{ title: "" }} />
      <Stack.Screen name="habit" options={{ title: "" }} />
      {/* The soul or memory editor names itself after the file. */}
      <Stack.Screen name="file" options={{ title: "" }} />
      <Stack.Screen name="model" options={{ title: "Model", ...androidBackHeader("Model") }} />
      <Stack.Screen
        name="model-options"
        options={{ title: "Options", ...androidBackHeader("Options") }}
      />
      <Stack.Screen
        name="projects"
        options={{ title: "Projects", ...androidBackHeader("Projects") }}
      />
    </Stack>
  );
}

import { router, Stack } from "expo-router";

import {
  HeaderButton,
  modalStackOptions,
  sheetScreenLayout,
} from "@/features/files/codeScreen";
import { useColors } from "@/ui/theme";

// The project picker: a native modal with its own stack and header, as
// settings is, its search in the header.
export default function ProjectsLayout() {
  const colors = useColors();
  return (
    <Stack
      screenOptions={modalStackOptions(colors)}
      screenLayout={sheetScreenLayout(colors.sheet)}
    >
      <Stack.Screen
        name="index"
        options={{
          title: "Projects",
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

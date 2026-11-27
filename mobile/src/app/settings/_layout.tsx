import { router, Stack } from "expo-router";

import {
  HeaderButton,
  modalStackOptions,
  sheetScreenLayout,
} from "@/features/files/codeScreen";
import { MONOS_ENABLED } from "@/features/connection/selection";
import { androidBackHeader } from "@/ui/AndroidHeader";
import { useColors } from "@/ui/theme";

// Settings: a native modal with its own stack, so its pages push inside the
// sheet and the close button dismisses the lot.
export default function SettingsLayout() {
  const colors = useColors();
  return (
    <Stack
      screenOptions={modalStackOptions(colors)}
      screenLayout={sheetScreenLayout(colors.sheet)}
    >
      <Stack.Screen
        name="index"
        options={{
          title: "Settings",
          headerLeft: () => (
            <HeaderButton
              icon="close"
              label="Close"
              onPress={() => router.back()}
            />
          ),
        }}
      />
      <Stack.Screen
        name="appearance"
        options={{ title: "Appearance", ...androidBackHeader("Appearance") }}
      />
      <Stack.Screen
        name="providers"
        options={{ title: "Providers", ...androidBackHeader("Providers") }}
      />
      <Stack.Screen
        name="permissions"
        options={{ title: "Permissions", ...androidBackHeader("Permissions") }}
      />
      {/* The provider's models title themselves. */}
      <Stack.Screen name="models" options={{ title: "" }} />
      <Stack.Screen
        name="workspace"
        options={{
          title: MONOS_ENABLED ? "Projects and monos" : "Projects",
          ...androidBackHeader(MONOS_ENABLED ? "Projects and monos" : "Projects"),
        }}
      />
    </Stack>
  );
}

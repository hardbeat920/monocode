import { router, Stack, useNavigation, type NativeStackNavigationOptions } from "expo-router";
import { Platform } from "react-native";

import {
  HeaderButton,
  modalStackOptions,
  sheetScreenLayout,
} from "@/features/files/codeScreen";
import { androidBackHeader } from "@/ui/AndroidHeader";
import { ModalSheet } from "@/ui/ModalSheet";
import { useColors } from "@/ui/theme";

// In Android's sheet the header starts at the sheet's top edge, under its
// handle, not a status bar's height below it.
const SHEET_HEADER: NativeStackNavigationOptions =
  Platform.OS === "android"
    ? { unstable_nativeProps: { headerConfig: { disableTopInsetApplication: true } } }
    : {};

// The sheet's first page titled as iOS titles it: centred, with no way back.
const SHEET_ROOT: NativeStackNavigationOptions =
  Platform.OS === "android" ? { headerTitleAlign: "center", headerBackVisible: false } : {};

// A Mono's profile: a modal sheet with its own stack, as Settings, so editing
// its look pushes inside the sheet and the close button dismisses the lot.
export default function MonoProfileLayout() {
  const colors = useColors();
  // The root stack's, so closing dismisses the whole sheet from any page.
  const navigation = useNavigation();
  return (
    <ModalSheet onClose={() => navigation.goBack()}>
      <Stack
        screenOptions={{ ...modalStackOptions(colors), ...SHEET_HEADER }}
        screenLayout={sheetScreenLayout(colors.sheet)}
      >
        <Stack.Screen
          name="index"
          options={{
            title: "",
            headerLeft: () => (
              <HeaderButton icon="close" label="Close" round onPress={() => router.back()} />
            ),
          }}
        />
        {/* Opened on its own for a new Mono, Edit is the sheet's first page. */}
        <Stack.Screen
          name="edit"
          options={({ navigation }) => ({
            title: "Edit",
            ...(navigation.getState().index > 0 ? androidBackHeader("Edit") : SHEET_ROOT),
          })}
        />
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
    </ModalSheet>
  );
}

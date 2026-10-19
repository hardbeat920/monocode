import {
  DarkTheme,
  DefaultTheme,
  Stack,
  ThemeProvider,
  type NativeStackNavigationOptions,
} from "expo-router";
import { StatusBar } from "expo-status-bar";
import { GestureHandlerRootView } from "react-native-gesture-handler";
import { KeyboardProvider } from "react-native-keyboard-controller";
import { SafeAreaProvider } from "react-native-safe-area-context";
import {
  ConnectionProvider,
  useConnection,
} from "@/features/connection/ConnectionProvider";
import { WorkspaceGate } from "@/features/connection/WorkspaceGate";

import { SessionNotifications } from "@/features/notifications/SessionNotifications";
import { SessionActivity } from "@/features/sessions/SessionActivity";
import { SessionFrame } from "@/features/sessions/SessionScreen";
import { WorkspaceProvider } from "@/features/workspace/WorkspaceProvider";
import { codeScreenOptions, HEADER_OWN_BLUR } from "@/features/files/codeScreen";
import { applySavedAppearance } from "@/features/settings/appearance";
import { ScenePagerHost } from "@/ui/ScenePager";
import {
  paletteFor,
  useScheme,
  useTheme,
  type Palette,
  type Scheme,
  type ThemeName,
} from "@/ui/theme";

// Light or dark as set in settings, or as the phone is.
applySavedAppearance();

const navigationTheme = (scheme: Scheme, colors: Palette) => {
  const base = scheme === "dark" ? DarkTheme : DefaultTheme;
  return {
    ...base,
    colors: {
      ...base.colors,
      background: colors.background,
      card: colors.background,
      primary: colors.accent,
      text: colors.content,
      border: colors.stroke,
    },
  };
};

// A project and its sessions: the native header, transparent over the
// screen's own gradual blur, which softens whatever is under it, scrolling or
// not, and fades out with no hard edge. Its buttons are native items, so they
// morph as a session is pushed.
const nativeHeader = (colors: Palette): NativeStackNavigationOptions => ({
  headerShown: true,
  headerTransparent: true,
  headerShadowVisible: false,
  headerTintColor: colors.content,
  headerBackButtonDisplayMode: "minimal",
  ...HEADER_OWN_BLUR,
  contentStyle: { backgroundColor: colors.background },
});

/** Every screen's options, in each scheme's colours. */
const screenOptions = (colors: Palette) => ({
  nativeHeader: nativeHeader(colors),
  // A session pushes over its project, with the stack's slide and swipe back.
  session: {
    ...nativeHeader(colors),
    animation: "default",
    gestureEnabled: true,
  } satisfies NativeStackNavigationOptions,
  // Full-screen modals with their own stacks and headers.
  modal: {
    presentation: "modal",
    animation: "default",
    contentStyle: { backgroundColor: colors.sheet },
  } satisfies NativeStackNavigationOptions,
  code: codeScreenOptions(colors),
  // Changes over a session: a code screen's modal, on the page's own colour
  // so its bottom blur matches.
  changes: {
    ...codeScreenOptions(colors),
    contentStyle: { backgroundColor: colors.background },
  } satisfies NativeStackNavigationOptions,
});

type NavigationLook = {
  navigation: ReturnType<typeof navigationTheme>;
  screens: ReturnType<typeof screenOptions>;
};

// Built the first time a scheme and theme are drawn, and kept, so the
// navigators see the same objects until either changes.
const looks = new Map<string, NavigationLook>();

/** The navigation theme and screen options of the scheme and theme in use. */
function useNavigationLook(): NavigationLook {
  const scheme = useScheme();
  const theme: ThemeName = useTheme();
  const key = `${theme}.${scheme}`;
  let look = looks.get(key);
  if (!look) {
    const colors = paletteFor(scheme, theme);
    look = {
      navigation: navigationTheme(scheme, colors),
      screens: screenOptions(colors),
    };
    looks.set(key, look);
  }
  return look;
}

export default function RootLayout() {
  const scheme = useScheme();
  return (
    <KeyboardProvider>
      <GestureHandlerRootView style={{ flex: 1 }}>
        <ThemeProvider value={useNavigationLook().navigation}>
          <SafeAreaProvider>
            <ConnectionProvider>
              <StatusBar style={scheme === "dark" ? "light" : "dark"} />
              <WorkspaceGate>
                <LinkedWorkspace />
              </WorkspaceGate>
            </ConnectionProvider>
          </SafeAreaProvider>
        </ThemeProvider>
      </GestureHandlerRootView>
    </KeyboardProvider>
  );
}

function LinkedWorkspace() {
  const connection = useConnection();
  const { screens } = useNavigationLook();
  return (
    <WorkspaceProvider key={connection.pairing?.desktop.deviceId}>
      <SessionNotifications />
      <SessionActivity />
      <ScenePagerHost>
        <SessionFrame>
          {/* A native stack: sessions push over their project, and the
              code screens, Changes, settings, the model picker and the project
              picker present
              over the rest as native modals; everything else swaps in place,
              as a plain slot did. */}
          <Stack screenOptions={{ headerShown: false, animation: "none" }}>
            <Stack.Screen name="file" options={screens.code} />
            <Stack.Screen name="diff" options={screens.code} />
            <Stack.Screen
              name="project/[projectId]/index"
              options={screens.nativeHeader}
            />
            <Stack.Screen
              name="project/[projectId]/session/[sessionId]"
              options={screens.session}
            />
            <Stack.Screen name="projects" options={screens.modal} />
            <Stack.Screen name="settings" options={screens.modal} />
            <Stack.Screen name="models" options={screens.modal} />
            <Stack.Screen name="changes" options={screens.changes} />
          </Stack>
        </SessionFrame>
      </ScenePagerHost>
    </WorkspaceProvider>
  );
}

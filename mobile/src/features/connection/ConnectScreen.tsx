import { CameraView, useCameraPermissions } from "expo-camera";
import { useRef, useState } from "react";
import {
  ActivityIndicator,
  Image,
  Modal,
  Pressable,
  StyleSheet,
  Text,
  View,
  type LayoutChangeEvent,
  type LayoutRectangle,
  type NativeSyntheticEvent,
  type TextLayoutEventData,
  type TextLayoutLine,
} from "react-native";
import Animated, {
  Easing,
  FadeIn,
  FadeInDown,
  useSharedValue,
} from "react-native-reanimated";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { MONOCODE_MARK } from "@/ui/brand";
import { LiquidView } from "@/ui/LiquidView";
import { themedStyles, useColors, withAlpha, type Palette } from "@/ui/theme";
import { AsciiField } from "@/features/background/AsciiField";
import { MascotRain } from "@/features/mascots/MascotRain";
import type { Collider } from "@/features/mascots/mascotPhysics";
import { useConnection } from "./ConnectionProvider";
import { PairingLinkSheet } from "./PairingLinkSheet";

const EASE = Easing.out(Easing.cubic);

// The first screen on a fresh install: a field of drifting ASCII waves that
// thins out before halfway down, the mark centred over it, and at the foot
// the pitch, to the left, above the ways to pair.
// Mascots rain down over it all, landing on the words and buttons, and tumble
// about as the phone tilts. In the light or dark palette, as the app is.
export function ConnectScreen() {
  const colors = useColors();
  const connectionStyles = useConnectionStyles();
  const styles = useStyles();
  const connection = useConnection();
  const insets = useSafeAreaInsets();
  const [permission, requestPermission] = useCameraPermissions();
  const [scanning, setScanning] = useState(false);
  const [pasting, setPasting] = useState(false);
  const [cameraError, setCameraError] = useState("");
  const scan = async () => {
    const result = permission?.granted ? permission : await requestPermission();
    if (result.granted) {
      setCameraError("");
      setScanning(true);
    } else
      setCameraError(
        "Allow camera access in your phone’s settings, or paste a pairing link.",
      );
  };
  const error = connection.error || cameraError;
  const colliders = useScreenColliders();
  return (
    <View style={styles.root}>
      <Animated.View
        pointerEvents="none"
        style={StyleSheet.absoluteFill}
        entering={FadeIn.duration(700).easing(EASE)}
      >
        <AsciiField />
      </Animated.View>
      <MascotRain colliders={colliders.shared} />
      <View
        style={[
          styles.content,
          { paddingTop: insets.top, paddingBottom: insets.bottom + 12 },
        ]}
      >
        <Animated.View
          style={styles.brand}
          entering={FadeIn.duration(700).delay(150).easing(EASE)}
        >
          <Image
            source={MONOCODE_MARK}
            style={styles.mark}
            tintColor={colors.content}
            accessibilityIgnoresInvertColors
          />
          <Text style={styles.wordmark}>Monocode</Text>
        </Animated.View>
        <Animated.View
          style={styles.intro}
          onLayout={colliders.layout("intro")}
          entering={FadeInDown.duration(700).delay(150).easing(EASE)}
        >
          <Text
            style={styles.title}
            onLayout={colliders.layout("title")}
            onTextLayout={colliders.lines("title")}
          >
            Keep coding,{"\n"}wherever you are.
          </Text>
          <Text
            style={styles.subtitle}
            onLayout={colliders.layout("subtitle")}
            onTextLayout={colliders.lines("subtitle")}
          >
            Pair this phone with Monocode on your desktop to pick up your
            sessions on the go.
          </Text>
        </Animated.View>
        <Animated.View
          style={styles.actions}
          onLayout={colliders.layout("actions")}
          entering={FadeInDown.duration(700).delay(300).easing(EASE)}
        >
          {connection.working ? (
            <ActivityIndicator color={colors.content} style={styles.working} />
          ) : null}
          {error ? <Text style={styles.error}>{error}</Text> : null}
          <View onLayout={colliders.layout("primary")}>
            {/* Glass in the theme's inverse: white on dark, black on light. */}
            <LiquidView
              onPress={() => void scan()}
              disabled={connection.working}
              accessibilityLabel="Connect to desktop"
              pressedScale={1.03}
              tintColor={colors.inverseSurface}
              style={styles.button}
              innerStyle={styles.buttonSurface}
            >
              <Text style={styles.primaryLabel}>Connect to desktop</Text>
            </LiquidView>
          </View>
          <View onLayout={colliders.layout("secondary")}>
            <LiquidView
              onPress={() => setPasting(true)}
              disabled={connection.working}
              accessibilityLabel="Paste a pairing link"
              pressedScale={1.03}
              style={styles.button}
              innerStyle={styles.buttonSurface}
            >
              <Text style={styles.secondaryLabel}>Paste a pairing link</Text>
            </LiquidView>
          </View>
        </Animated.View>
      </View>
      <PairingLinkSheet
        isPresented={pasting}
        onDismiss={() => setPasting(false)}
        onConnect={(link) => {
          setPasting(false);
          void connection.pair(link);
        }}
      />
      <Modal
        visible={scanning}
        animationType="slide"
        onRequestClose={() => setScanning(false)}
      >
        <View style={{ flex: 1, backgroundColor: colors.background }}>
          {scanning ? (
            <CameraView
              style={{ flex: 1 }}
              barcodeScannerSettings={{ barcodeTypes: ["qr"] }}
              onBarcodeScanned={({ data }) => {
                setScanning(false);
                void connection.pair(data);
              }}
            />
          ) : null}
          <View style={{ padding: 24, paddingBottom: insets.bottom + 24 }}>
            <Text style={[connectionStyles.subtitle, { textAlign: "center" }]}>
              Scan the code in desktop Settings → Connections
            </Text>
            <Pressable onPress={() => setScanning(false)}>
              <Text style={connectionStyles.secondary}>Cancel</Text>
            </Pressable>
          </View>
        </View>
      </Modal>
    </View>
  );
}

type Part =
  "intro" | "title" | "subtitle" | "actions" | "primary" | "secondary";

/**
 * What the mascots land on, in screen points: each line of the words, from
 * the tops of its capitals to just under its baseline, and the buttons as
 * pills. Put together from where each part is laid out in its parent: the
 * words in the intro, the buttons in the actions, both in the content, which
 * fills the screen from its top-left corner.
 */
function useScreenColliders() {
  const shared = useSharedValue<Collider[]>([]);
  const frames = useRef<Partial<Record<Part, LayoutRectangle>>>({});
  const texts = useRef<Partial<Record<Part, TextLayoutLine[]>>>({});
  const sync = () => {
    const { intro, actions, primary, secondary } = frames.current;
    const out: Collider[] = [];
    if (intro)
      for (const part of ["title", "subtitle"] as const) {
        const frame = frames.current[part];
        if (!frame) continue;
        for (const line of texts.current[part] ?? []) {
          if (!line.text.trim()) continue;
          // On iOS a line's ascender is its baseline, down from its top.
          const baseline = line.y + line.ascender;
          out.push({
            x: intro.x + frame.x + line.x,
            y: intro.y + frame.y + baseline - line.capHeight,
            width: line.width,
            height: line.capHeight + 3,
            radius: 2,
          });
        }
      }
    if (actions)
      for (const frame of [primary, secondary])
        if (frame)
          out.push({
            x: actions.x + frame.x,
            y: actions.y + frame.y,
            width: frame.width,
            height: frame.height,
            radius: frame.height / 2,
          });
    shared.set(out);
  };
  return {
    shared,
    layout: (part: Part) => (event: LayoutChangeEvent) => {
      frames.current[part] = event.nativeEvent.layout;
      sync();
    },
    lines:
      (part: Part) => (event: NativeSyntheticEvent<TextLayoutEventData>) => {
        texts.current[part] = event.nativeEvent.lines;
        sync();
      },
  };
}

const useStyles = themedStyles((colors) => ({
  root: {
    flex: 1,
    backgroundColor: colors.background,
  },
  content: {
    flex: 1,
    paddingHorizontal: 28,
  },
  // At the head, over the field.
  brand: {
    alignItems: "center",
    paddingTop: 8,
  },
  // Pushed to the foot, under the field, just above the buttons.
  intro: {
    flex: 1,
    alignItems: "flex-start",
    justifyContent: "flex-end",
    paddingBottom: 40,
  },
  mark: {
    width: 44,
    height: 44,
  },
  wordmark: {
    marginTop: 8,
    color: withAlpha(colors.content, 0.8),
    fontSize: 15,
    fontWeight: "600",
  },
  title: {
    color: colors.content,
    fontSize: 38,
    lineHeight: 44,
    fontWeight: "700",
  },
  subtitle: {
    marginTop: 14,
    maxWidth: 320,
    color: withAlpha(colors.content, 0.78),
    fontSize: 16,
    lineHeight: 23,
  },
  actions: {
    gap: 12,
  },
  working: {
    marginBottom: 4,
  },
  error: {
    color: colors.deletions,
    fontSize: 14,
    lineHeight: 20,
    marginBottom: 4,
  },
  primaryLabel: { color: colors.inverse, fontSize: 16, fontWeight: "600" },
  // Both buttons: full width, a pill's height.
  button: {
    alignSelf: "stretch",
  },
  buttonSurface: {
    minHeight: 54,
    paddingVertical: 16,
    justifyContent: "center",
  },
  secondaryLabel: { color: colors.content, fontSize: 16, fontWeight: "600" },
}));

/** The connection screens' shared styles, in a palette's colours. */
export const connectionStyleSheet = (colors: Palette) =>
  StyleSheet.create({
    screen: {
      flex: 1,
      backgroundColor: colors.background,
      paddingHorizontal: 28,
    },
    title: {
      color: colors.content,
      fontSize: 34,
      lineHeight: 40,
      fontWeight: "600",
    },
    subtitle: { color: colors.muted, fontSize: 16, lineHeight: 24 },
    primary: {
      backgroundColor: colors.inverseSurface,
      borderRadius: 32,
      padding: 17,
      alignItems: "center",
    },
    primaryLabel: { color: colors.inverse, fontSize: 16, fontWeight: "600" },
    secondary: {
      color: colors.accent,
      textAlign: "center",
      padding: 14,
      fontSize: 16,
    },
    error: {
      color: colors.deletions,
      fontSize: 14,
      lineHeight: 21,
      marginBottom: 16,
    },
  });

export const useConnectionStyles = themedStyles(connectionStyleSheet);

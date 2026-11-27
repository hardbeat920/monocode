import * as Haptics from "expo-haptics";
import { router, Stack } from "expo-router";
import { useState } from "react";
import {
  ActivityIndicator,
  Alert,
  Pressable,
  ScrollView,
  Text,
  TextInput,
  View,
} from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { useConnection } from "@/features/connection/ConnectionProvider";
import { PixelMascot } from "@/features/mascots/PixelMascot";
import { PIXEL_MASCOT_NAMES } from "@/features/mascots/pixelMascots";
import { SWATCHES } from "@/features/workspace/monoLook";
import { monoCreated } from "@/features/workspace/newMono";
import { HeaderBlur, HeaderButton } from "@/features/files/codeScreen";
import { GradualBlur } from "@/ui/GradualBlur";
import { radii, themedStyles, useColors, useScheme } from "@/ui/theme";

const COLUMNS = 5;
const TILE_GAP = 8;
const BUTTON_HEIGHT = 54;
/** How far the blur under the Create button keeps fading above it. */
const FOOT_FADE = 24;

const pick = <T,>(items: readonly T[]) => items[Math.floor(Math.random() * items.length)];

// A new Mono, made in one go as a messaging app makes a contact: its look
// picked here, a name given, then Create makes it on the desktop with that
// look and opens its chat. Opened from + on the Mono list and in the drawer.
export default function NewMonoScreen() {
  const { command, refresh, selection, select } = useConnection();
  const insets = useSafeAreaInsets();
  const styles = useStyles();
  const colors = useColors();
  const scheme = useScheme();
  const [name, setName] = useState("");
  // A fresh look each time, for the user to keep or change.
  const [mascot, setMascot] = useState(() => pick(PIXEL_MASCOT_NAMES));
  const [color, setColor] = useState(() => pick(SWATCHES));
  const [creating, setCreating] = useState(false);
  // Five square tiles to a row, sized to the grid's width.
  const [gridWidth, setGridWidth] = useState(0);
  const tile = gridWidth > 0 ? Math.floor((gridWidth - TILE_GAP * (COLUMNS - 1)) / COLUMNS) : 0;
  const ready = name.trim().length > 0 && !creating;

  const create = async () => {
    if (!ready) return;
    setCreating(true);
    try {
      const { monoId } = await command<{ monoId: string }>("mono.create");
      await command("mono.update", { monoId, name: name.trim(), mascot, color });
      // Shown on this phone at once.
      if (selection) await select({ ...selection, monoIds: [...selection.monoIds, monoId] });
      await refresh();
      void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
      // The sheet goes, and the screen under it opens the new Mono's chat.
      router.back();
      monoCreated(monoId);
    } catch (reason) {
      setCreating(false);
      Alert.alert(
        "Couldn’t create a Mono",
        reason instanceof Error ? reason.message : String(reason),
      );
    }
  };

  return (
    <View style={styles.container}>
      <ScrollView
        // Clear of the native header, which it scrolls under.
        contentInsetAdjustmentBehavior="automatic"
        contentContainerStyle={[
          styles.content,
          { paddingBottom: BUTTON_HEIGHT + Math.max(insets.bottom, 16) + 32 },
        ]}
        keyboardShouldPersistTaps="handled"
        keyboardDismissMode="on-drag"
        showsVerticalScrollIndicator={false}
      >
        <View style={styles.preview}>
          <PixelMascot name={mascot} color={color} size={120} centered />
        </View>

        <TextInput
          value={name}
          onChangeText={setName}
          maxLength={40}
          placeholder="Name your Mono"
          placeholderTextColor={colors.muted}
          keyboardAppearance={scheme}
          returnKeyType="done"
          onSubmitEditing={() => void create()}
          editable={!creating}
          style={styles.input}
        />

        <View
          style={styles.grid}
          onLayout={(event) => setGridWidth(event.nativeEvent.layout.width)}
        >
          {tile > 0 &&
            PIXEL_MASCOT_NAMES.map((option) => {
              const selected = option === mascot;
              return (
                <Pressable
                  key={option}
                  accessibilityRole="radio"
                  accessibilityLabel={option}
                  accessibilityState={{ selected }}
                  onPress={() => {
                    if (selected) return;
                    void Haptics.selectionAsync();
                    setMascot(option);
                  }}
                  style={[
                    styles.tile,
                    { width: tile, height: tile },
                    selected && { borderColor: colors.muted },
                  ]}
                >
                  <PixelMascot name={option} color={color} size={Math.round(tile * 0.6)} centered />
                </Pressable>
              );
            })}
        </View>

        <View style={styles.swatches}>
          {SWATCHES.map((swatch) => {
            const selected = swatch === color;
            return (
              <Pressable
                key={swatch}
                accessibilityRole="radio"
                accessibilityLabel={swatch}
                accessibilityState={{ selected }}
                hitSlop={4}
                onPress={() => {
                  if (selected) return;
                  void Haptics.selectionAsync();
                  setColor(swatch);
                }}
                style={[styles.ring, selected && { borderColor: colors.muted }]}
              >
                <View style={[styles.dot, { backgroundColor: swatch }]} />
              </Pressable>
            );
          })}
        </View>
      </ScrollView>

      {/* The native header, transparent over the sheet as Settings' is,
          with close on the left. */}
      <Stack.Screen
        options={{
          title: "New Mono",
          headerLeft: () => (
            <HeaderButton icon="close" label="Close" onPress={() => router.back()} />
          ),
        }}
      />

      {/* The same gradual blur under the header as every sheet, and one
          under the Create button for the content scrolling past it. */}
      <HeaderBlur color={colors.sheet} />
      <GradualBlur
        edge="bottom"
        height={BUTTON_HEIGHT + Math.max(insets.bottom, 16) + 12 + FOOT_FADE}
        color={colors.sheet}
      />

      <View style={[styles.foot, { paddingBottom: Math.max(insets.bottom, 16) }]}>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Create"
          accessibilityState={{ disabled: !ready }}
          disabled={!ready}
          onPress={() => void create()}
          style={({ pressed }) => [
            styles.button,
            { backgroundColor: ready ? colors.content : colors.surface },
            pressed && styles.pressed,
          ]}
        >
          {creating ? (
            <ActivityIndicator color={colors.background} />
          ) : (
            <Text style={[styles.buttonLabel, { color: ready ? colors.background : colors.muted }]}>
              Create
            </Text>
          )}
        </Pressable>
      </View>
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  container: {
    flex: 1,
    backgroundColor: colors.sheet,
  },
  content: {
    paddingHorizontal: 20,
    gap: 28,
  },
  preview: {
    alignItems: "center",
    paddingTop: 20,
  },
  input: {
    height: 56,
    borderRadius: radii.card,
    borderCurve: "continuous",
    backgroundColor: colors.surface,
    color: colors.content,
    fontSize: 20,
    textAlign: "center",
    paddingHorizontal: 16,
  },
  grid: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: TILE_GAP,
  },
  tile: {
    alignItems: "center",
    justifyContent: "center",
    borderRadius: 16,
    borderCurve: "continuous",
    borderWidth: 2,
    borderColor: "transparent",
  },
  swatches: {
    flexDirection: "row",
    flexWrap: "wrap",
    justifyContent: "center",
    gap: 14,
  },
  ring: {
    width: 40,
    height: 40,
    borderRadius: 20,
    borderWidth: 2,
    borderColor: "transparent",
    alignItems: "center",
    justifyContent: "center",
  },
  dot: {
    width: 30,
    height: 30,
    borderRadius: 15,
  },
  foot: {
    position: "absolute",
    left: 0,
    right: 0,
    bottom: 0,
    paddingHorizontal: 24,
    paddingTop: 12,
  },
  button: {
    height: BUTTON_HEIGHT,
    borderRadius: BUTTON_HEIGHT / 2,
    borderCurve: "continuous",
    alignItems: "center",
    justifyContent: "center",
  },
  buttonLabel: {
    fontSize: 17,
    fontWeight: "600",
  },
  pressed: {
    opacity: 0.8,
  },
}));

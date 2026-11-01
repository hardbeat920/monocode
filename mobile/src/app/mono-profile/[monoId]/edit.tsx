import * as Haptics from "expo-haptics";
import { router, useLocalSearchParams } from "expo-router";
import { useEffect, useRef, useState } from "react";
import { Alert, Pressable, StyleSheet, Text, TextInput, View } from "react-native";

import { PixelMascot } from "@/features/mascots/PixelMascot";
import { PIXEL_MASCOT_NAMES } from "@/features/mascots/pixelMascots";
import { useMonoModel } from "@/features/models/useMonoModel";
import { ProviderLogo } from "@/features/sessions/ProviderLogo";
import { SettingsPage, SettingsRow } from "@/features/settings/SettingsUI";
import { useOffline } from "@/features/connection/ConnectionNotice";
import { useMonoFiles } from "@/features/connection/ConnectionProvider";
import { useWorkspace, type MonoChange } from "@/features/workspace/WorkspaceProvider";
import { Icon } from "@/ui/Icon";
import { setMonoAccent, useMonoAccent, type MonoAccent } from "@/features/workspace/monoAccent";
import { ACCENTS, paletteFor, radii, themedStyles, useColors, useScheme } from "@/ui/theme";

const TILE_GAP = 10;

/** The desktop's swatches for a Mono (COLORS_MONO). */
const SWATCHES = [
  "#459cf7", "#ea603e", "#f4bf25", "#39c363", "#e45a9f", "#b168d4", "#37beb4", "#ef8c39", "#8b63e9",
];

// A Mono's look: its name, then which mascot it is and in what colour, as the
// desktop's look editor. Each change shows everywhere at once and saves on the
// desktop behind it; without the desktop there it can't be changed.
export default function EditMonoScreen() {
  const { monoId } = useLocalSearchParams<{ monoId: string }>();
  const { monos, editMono } = useWorkspace();
  const mono = monos.find((m) => m.id === monoId);
  const offline = useOffline();
  const { model, harness } = useMonoModel(mono);
  // The older desktop app can't hand the phone its soul and memory.
  const files = useMonoFiles();
  const styles = useStyles();
  const colors = useColors();
  const scheme = useScheme();
  const [name, setName] = useState(mono?.name ?? "");
  // The mascot grid's inner width: five square tiles to a row, sized exactly
  // so each figure sits in the middle of a true square.
  const [gridWidth, setGridWidth] = useState(0);
  const tile = gridWidth > 0 ? Math.floor((gridWidth - TILE_GAP * 4) / 5) : 0;
  const saved = useRef(mono?.name ?? "");
  const mascot = mono?.mascot ?? "ghost";
  const color = mono?.color ?? SWATCHES[0];

  const save = (change: MonoChange) => {
    if (!mono) return;
    editMono(mono, change).catch((reason: unknown) =>
      Alert.alert("Couldn’t save", reason instanceof Error ? reason.message : String(reason)),
    );
  };
  // The name saves once typing pauses, not on every key, or as the screen
  // closes if that comes first. A cleared name leaves the saved one.
  const pending = useRef<string | null>(null);
  const flush = useRef(() => {});
  useEffect(() => {
    flush.current = () => {
      const next = pending.current;
      pending.current = null;
      if (next === null) return;
      saved.current = next;
      save({ name: next });
    };
  });
  useEffect(() => {
    const next = name.trim();
    if (!next || next === saved.current) {
      pending.current = null;
      return;
    }
    pending.current = next;
    const timer = setTimeout(() => flush.current(), 600);
    return () => clearTimeout(timer);
  }, [name]);
  useEffect(() => () => flush.current(), []);

  if (!mono) return null;
  return (
    <SettingsPage>
      <View style={styles.preview}>
        <PixelMascot name={mascot} color={color} size={88} centered />
      </View>

      <Text style={styles.heading}>Name</Text>
      <View style={styles.card}>
        <TextInput
          value={name}
          onChangeText={setName}
          maxLength={40}
          placeholder="Name"
          placeholderTextColor={colors.muted}
          keyboardAppearance={scheme}
          returnKeyType="done"
          editable={!offline}
          style={[styles.input, offline && styles.off]}
        />
      </View>

      <Text style={styles.heading}>Model</Text>
      <View style={[styles.card, offline && styles.off]} pointerEvents={offline ? "none" : "auto"}>
        <SettingsRow
          icon={harness ? <ProviderLogo harness={harness} size={18} /> : "sparkle"}
          label={model || "Choose a model"}
          onPress={() => router.push({ pathname: "/mono-profile/[monoId]/model", params: { monoId } })}
        />
      </View>

      {files ? (
        <>
          <Text style={styles.heading}>Mind</Text>
          <View style={[styles.card, offline && styles.off]} pointerEvents={offline ? "none" : "auto"}>
            <SettingsRow
              icon="sparkle"
              label="Soul"
              onPress={() =>
                router.push({ pathname: "/mono-profile/[monoId]/file", params: { monoId, file: "soul" } })
              }
            />
            <View style={styles.divider} />
            <SettingsRow
              icon="note"
              label="Memory"
              onPress={() =>
                router.push({ pathname: "/mono-profile/[monoId]/file", params: { monoId, file: "memory" } })
              }
            />
          </View>
        </>
      ) : null}

      <Text style={styles.heading}>Mascot</Text>
      <View
        style={[styles.card, styles.grid, offline && styles.off]}
        pointerEvents={offline ? "none" : "auto"}
        onLayout={(event) => setGridWidth(event.nativeEvent.layout.width - 24)}
      >
        {tile > 0 && PIXEL_MASCOT_NAMES.map((name) => (
          <Pressable
            key={name}
            accessibilityRole="radio"
            accessibilityLabel={name}
            accessibilityState={{ selected: name === mascot }}
            onPress={() => {
              if (name === mascot) return;
              void Haptics.selectionAsync();
              save({ mascot: name });
            }}
            style={[styles.mascot, { width: tile, height: tile }, name === mascot && { borderColor: color }]}
          >
            <PixelMascot name={name} color={color} size={Math.round(tile * 0.62)} centered />
          </Pressable>
        ))}
      </View>

      <Text style={styles.heading}>Color</Text>
      <View style={[styles.card, styles.swatches, offline && styles.off]} pointerEvents={offline ? "none" : "auto"}>
        {SWATCHES.map((swatch) => (
          <Pressable
            key={swatch}
            accessibilityRole="radio"
            accessibilityLabel={swatch}
            accessibilityState={{ selected: swatch === color.toLowerCase() }}
            hitSlop={4}
            onPress={() => {
              if (swatch === color.toLowerCase()) return;
              void Haptics.selectionAsync();
              save({ color: swatch });
            }}
            style={[styles.ring, swatch === color.toLowerCase() && { borderColor: swatch }]}
          >
            <View style={[styles.dot, { backgroundColor: swatch }]}>
              {swatch === color.toLowerCase() ? <Icon name="check" size={14} color="#ffffff" /> : null}
            </View>
          </Pressable>
        ))}
      </View>

      <ChatColor monoId={monoId} />
      {offline ? (
        <Text style={styles.note}>Connect to your desktop to change this Mono.</Text>
      ) : null}
    </SettingsPage>
  );
}

const CHAT_ACCENTS: MonoAccent[] = ["theme", ...(Object.keys(ACCENTS) as MonoAccent[])];

/** The colour this Mono's chat is drawn in, over the app's accent: its
 *  bubbles, links and send button. Kept on this phone, so it needs no desktop. */
function ChatColor({ monoId }: { monoId: string }) {
  const styles = useStyles();
  const scheme = useScheme();
  const picked = useMonoAccent(monoId);
  return (
    <>
      <Text style={styles.heading}>Chat color</Text>
      <View style={[styles.card, styles.swatches]}>
        {CHAT_ACCENTS.map((accent) => {
          const fill = accent === "theme" ? paletteFor(scheme).accent : ACCENTS[accent][scheme];
          const selected = accent === picked;
          return (
            <Pressable
              key={accent}
              accessibilityRole="radio"
              accessibilityLabel={accent === "theme" ? "App accent" : ACCENTS[accent].label}
              accessibilityState={{ selected }}
              hitSlop={4}
              onPress={() => {
                if (selected) return;
                void Haptics.selectionAsync();
                setMonoAccent(monoId, accent);
              }}
              style={[styles.ring, selected && { borderColor: fill }]}
            >
              <View style={[styles.dot, { backgroundColor: fill }]}>
                {accent === "theme" ? (
                  <View style={styles.themeMark} />
                ) : selected ? (
                  <Icon name="check" size={14} color="#ffffff" />
                ) : null}
              </View>
            </Pressable>
          );
        })}
      </View>
      <Text style={styles.note}>
        {picked === "theme"
          ? "This chat uses the app's accent."
          : `This chat's bubbles and links are ${ACCENTS[picked].label.toLowerCase()}, whatever the app's accent.`}
      </Text>
    </>
  );
}

const useStyles = themedStyles((colors) => ({
  // The app's own accent: a small hollow centre, so it reads as "default".
  themeMark: {
    width: 10,
    height: 10,
    borderRadius: 5,
    borderWidth: 2,
    borderColor: "#ffffff",
  },
  off: {
    opacity: 0.4,
  },
  divider: {
    height: StyleSheet.hairlineWidth,
    marginLeft: 44,
    backgroundColor: colors.stroke,
  },
  note: {
    color: colors.muted,
    fontSize: 13,
    lineHeight: 18,
    marginTop: 12,
    marginHorizontal: 16,
  },
  preview: {
    alignItems: "center",
    paddingVertical: 8,
  },
  heading: {
    color: colors.muted,
    fontSize: 15,
    marginTop: 24,
    marginBottom: 8,
    marginLeft: 16,
  },
  card: {
    backgroundColor: colors.surface,
    borderRadius: radii.card,
    borderCurve: "continuous",
    paddingHorizontal: 10,
    paddingVertical: 6,
  },
  input: {
    color: colors.content,
    fontSize: 17,
    paddingHorizontal: 8,
    paddingVertical: 12,
  },
  grid: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: TILE_GAP,
    paddingHorizontal: 12,
    paddingVertical: 12,
  },
  mascot: {
    alignItems: "center",
    justifyContent: "center",
    borderRadius: 16,
    borderCurve: "continuous",
    borderWidth: 2,
    borderColor: "transparent",
    backgroundColor: colors.background,
  },
  swatches: {
    flexDirection: "row",
    flexWrap: "wrap",
    justifyContent: "space-between",
    paddingHorizontal: 12,
    paddingVertical: 12,
    rowGap: 10,
  },
  ring: {
    width: 36,
    height: 36,
    borderRadius: 18,
    borderWidth: 2,
    borderColor: "transparent",
    alignItems: "center",
    justifyContent: "center",
  },
  dot: {
    width: 26,
    height: 26,
    borderRadius: 13,
    alignItems: "center",
    justifyContent: "center",
  },
}));

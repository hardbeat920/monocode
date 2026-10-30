import { memo, useMemo, useState } from "react";
import { StyleSheet, Text, View, type TextStyle } from "react-native";
import Animated, {
  useAnimatedStyle,
  type SharedValue,
} from "react-native-reanimated";
import { FileTypeIcon } from "@/features/files/FileTypeIcon";
import { useColors, useScheme } from "@/ui/theme";
import {
  TAG_ICON_SLOT,
  mentionIconPrefixes,
  type MentionLine,
  type MentionPart,
} from "./composerMentions";

/** Shared with TextInput so wrapping, caret positions and scroll stay aligned. */
export const COMPOSER_TEXT_METRICS: TextStyle = {
  fontSize: 17,
  lineHeight: 22,
  paddingHorizontal: 0,
  includeFontPadding: false,
};
const ICON = 13;

/** Desktop-style paint layer. The native TextInput still owns text, selection,
 * accessibility and all touches. A blank icon slot keeps a stable native advance width. */
export const ComposerMentionText = memo(function ComposerMentionText({
  text,
  parts,
  scroll,
  iconsOnly = false,
}: {
  text: string;
  parts: MentionPart[];
  scroll: SharedValue<{ x: number; y: number }>;
  iconsOnly?: boolean;
}) {
  const colors = useColors();
  const [layout, setLayout] = useState<{ text: string; lines: MentionLine[] }>({
    text: "",
    lines: [],
  });
  const [slotAndProbe, setSlotAndProbe] = useState(0);
  const [probeWidth, setProbeWidth] = useState(0);
  const atWidth = slotAndProbe - probeWidth;
  const prefixes = useMemo(() => {
    // Typing after a tag does not change its position. Keep its native icon
    // mounted while the next line measurement arrives.
    const stable =
      layout.text === text
        ? parts
        : parts.filter(
            (part) =>
              !part.file ||
              layout.text.slice(0, part.start + part.text.length) ===
                text.slice(0, part.start + part.text.length),
          );
    return mentionIconPrefixes(text, stable, layout.lines);
  }, [layout, text, parts]);
  const offset = useAnimatedStyle(() => ({
    transform: [
      { translateX: -scroll.get().x },
      { translateY: -scroll.get().y },
    ],
  }));
  return (
    <View
      pointerEvents="none"
      accessible={false}
      accessibilityElementsHidden
      importantForAccessibility="no-hide-descendants"
      style={styles.clip}
    >
      <Animated.View style={[styles.content, offset]}>
        <Text
          style={[
            COMPOSER_TEXT_METRICS,
            { color: colors.content, opacity: iconsOnly ? 0 : 1 },
          ]}
          textBreakStrategy="simple"
          lineBreakStrategyIOS="none"
          onTextLayout={(event) => {
            const lines = event.nativeEvent.lines.map(
              ({ text, x, y, height }) => ({ text, x, y, height }),
            );
            setLayout((current) =>
              current.text === text &&
              JSON.stringify(current.lines) === JSON.stringify(lines)
                ? current
                : { text, lines },
            );
          }}
        >
          <ComposerTextRuns parts={parts} />
        </Text>
        <Text
          style={[COMPOSER_TEXT_METRICS, styles.measure]}
          onTextLayout={(event) =>
            setSlotAndProbe(event.nativeEvent.lines[0]?.width ?? 0)
          }
        >
          {TAG_ICON_SLOT}M
        </Text>
        <Text
          style={[COMPOSER_TEXT_METRICS, styles.measure]}
          onTextLayout={(event) =>
            setProbeWidth(event.nativeEvent.lines[0]?.width ?? 0)
          }
        >
          M
        </Text>
        {prefixes.map((marker) => (
          <MentionIcon
            key={`${marker.start}\n${marker.file.path}\n${marker.prefix}\n${marker.x}\n${marker.y}`}
            marker={marker}
            atWidth={atWidth}
            probeWidth={probeWidth}
          />
        ))}
      </Animated.View>
    </View>
  );
});

/** Android paints styled spans in EditText itself, so IME composition never
 * competes with a second visible copy of its text. No value+children pairing. */
export function ComposerTextRuns({ parts }: { parts: MentionPart[] }) {
  const scheme = useScheme();
  return (
    <Text style={{ backgroundColor: "transparent" }}>
      {parts.map((part) =>
        part.file ? (
          <Text
            key={part.start}
            style={{
              color: scheme === "dark" ? "#38bdf8" : "#0284c7",
              backgroundColor: "transparent",
            }}
          >
            <Text>{TAG_ICON_SLOT}</Text>
            {part.text.slice(1)}
          </Text>
        ) : (
          part.text
        ),
      )}
    </Text>
  );
}

function MentionIcon({
  marker,
  atWidth,
  probeWidth,
}: {
  marker: ReturnType<typeof mentionIconPrefixes>[number];
  atWidth: number;
  probeWidth: number;
}) {
  const [width, setWidth] = useState<number>();
  return (
    <>
      <Text
        numberOfLines={1}
        style={[COMPOSER_TEXT_METRICS, styles.measure]}
        onTextLayout={(event) => setWidth(event.nativeEvent.lines[0]?.width)}
      >
        {marker.prefix}M
      </Text>
      {width !== undefined && atWidth > 0 ? (
        <View
          style={{
            position: "absolute",
            left: marker.x + width - probeWidth - atWidth / 2 - ICON / 2,
            top: marker.y + (marker.height - ICON) / 2,
            width: ICON,
            height: ICON,
          }}
        >
          <FileTypeIcon
            name={marker.file.name}
            isDir={marker.file.isDir}
            size={ICON}
          />
        </View>
      ) : null}
    </>
  );
}
const styles = StyleSheet.create({
  clip: {
    position: "absolute",
    top: 0,
    bottom: 0,
    left: 0,
    right: 0,
    overflow: "hidden",
  },
  content: { position: "absolute", top: 8, left: 0, right: 0 },
  measure: { position: "absolute", top: 0, left: 0, right: 0, opacity: 0 },
});

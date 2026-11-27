import { Text, View } from "react-native";
import Svg, { Path } from "react-native-svg";

import { mascotPath } from "@/features/mascots/projectMascots";
import { themedStyles, useColors, withAlpha } from "@/ui/theme";

const GRID_W = 16;

/**
 * A page with a folded corner and a question mark, in the '#'/'.' pixel
 * convention of the desktop's empty session list (SessionsEmpty). `GLOW` is
 * a sparse scatter drawn dimmer so the shape dissolves at its edges.
 */
const PAGE = [
  "...#######......",
  "...#.....##.....",
  "...#.....#.#....",
  "...#.....####...",
  "...#........#...",
  "...#...##...#...",
  "...#..#..#..#...",
  "...#.....#..#...",
  "...#....#...#...",
  "...#........#...",
  "...#....#...#...",
  "...#........#...",
  "...##########...",
];

const GLOW = [
  "#.#..........#.#",
  "................",
  "#..............#",
  "................",
  "................",
  "................",
  "................",
  "................",
  "................",
  "................",
  "#..............#",
  "................",
  "#.#..........#.#",
];

const PAGE_PATH = mascotPath(PAGE);
const GLOW_PATH = mascotPath(GLOW);

/** Why a file couldn't be shown, centred under a pixel page. */
export function FileMissing({ message }: { message: string }) {
  const styles = useStyles();
  const colors = useColors();
  const fill = withAlpha(colors.content, 0.25);
  return (
    // Centred in the whole modal, under the transparent header too, so it
    // sits in the middle of the sheet rather than of the space below it.
    <View style={styles.root}>
      <Svg
        width={96}
        height={(96 * PAGE.length) / GRID_W}
        viewBox={`0 0 ${GRID_W} ${PAGE.length}`}
        accessibilityElementsHidden
      >
        <Path d={GLOW_PATH} fill={fill} opacity={0.4} />
        <Path d={PAGE_PATH} fill={fill} />
      </Svg>
      <Text style={styles.message}>{message}</Text>
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  root: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
    gap: 20,
    paddingHorizontal: 24,
  },
  message: {
    color: colors.muted,
    fontSize: 15,
    textAlign: "center",
  },
}));

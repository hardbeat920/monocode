import {
  memo,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactElement,
} from "react";
import {
  FlatList,
  ScrollView,
  Text,
  useWindowDimensions,
  View,
} from "react-native";
import Animated, {
  scrollTo,
  useAnimatedRef,
  useAnimatedScrollHandler,
  useSharedValue,
} from "react-native-reanimated";

import { useHeaderHeight } from "expo-router/react-navigation";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { MONO } from "@/features/changes/diffTheme";
import { themedStyles, useScheme, withAlpha } from "@/ui/theme";

/** A diff line's colours: its row, its gutter and its number and sign. */
export type Tone = { row: string; gutter: string; fg: string };

// The desktop highlights in github-dark; in light mode each of its colours is
// swapped for github-light's, which share its scopes one to one.
const GITHUB_LIGHT: Record<string, string> = {
  "#e1e4e8": "#24292e",
  "#6a737d": "#6a737d",
  "#79b8ff": "#005cc5",
  "#b392f0": "#6f42c1",
  "#85e89d": "#22863a",
  "#f97583": "#d73a49",
  "#9ecbff": "#032f62",
  "#ffab70": "#e36209",
  "#fdaeb7": "#b31d28",
  "#dbedff": "#032f62",
  "#d1d5da": "#586069",
};

/** One row of code: a line of a file, or of a diff. */
export type CodeRow = {
  /** The line number in the gutter, if it has one. */
  number: number | null;
  text: string;
  /** Its syntax colours as runs of [text, index into the palette]. */
  runs?: [string, number][];
  /** A diff's +, − or space, beside the number. */
  sign?: string;
  tone?: Tone | null;
  /** A diff's unchanged line, drawn softer than its changes. */
  dim?: boolean;
  /** A diff's hunk header, across the whole row. */
  hunk?: boolean;
};

const ROW_HEIGHT = 20;
const HUNK_HEIGHT = 26;
const NUMBER_WIDTH = 44;
const SIGN_WIDTH = 20;
/** Menlo's advance at the code's font size, to size the sideways scroll. */
const CHAR_WIDTH = 7.23;
/** Lines are cut off past this many characters rather than scroll further. */
const MAX_COLUMNS = 1_000;

type ListProps = {
  rows: CodeRow[];
  palette?: string[];
  signs: boolean;
  footer?: ReactElement | null;
};

/**
 * Lines of code in a virtualised list. Without
 * wrapping, the code sits in a native sideways scroll view of its own, so a
 * sideways scroll moves it as one layer however long the file, and the line
 * numbers (and a diff's signs) are a column beside it that follows the code
 * down but never sideways. With `wrap`, long lines wrap under their number in
 * one list instead. Unwrapped rows are a fixed height, so a long file opens
 * and scrolls without drawing every line.
 */
export function CodeView({
  rows,
  palette,
  wrap,
  resetKey,
  footer,
}: {
  rows: CodeRow[];
  palette?: string[];
  wrap: boolean;
  /** The sideways scroll goes back to the start when this changes, e.g. another file. */
  resetKey: string;
  footer?: ReactElement | null;
}) {
  const signs = rows.some((row) => row.sign !== undefined);
  const light = useScheme() === "light";
  const shown = useMemo(
    () =>
      light
        ? palette?.map((color) => GITHUB_LIGHT[color.toLowerCase()] ?? color)
        : palette,
    [palette, light],
  );
  return wrap ? (
    <WrappedList rows={rows} palette={shown} signs={signs} footer={footer} />
  ) : (
    <PannedList
      rows={rows}
      palette={shown}
      signs={signs}
      footer={footer}
      resetKey={resetKey}
    />
  );
}

function WrappedList({ rows, palette, signs, footer }: ListProps) {
  const styles = useStyles();
  return (
    <FlatList
      style={styles.list}
      // Clear of the screen's transparent header and the home indicator,
      // and the scroll view iOS blurs under the header.
      contentInsetAdjustmentBehavior="automatic"
      automaticallyAdjustsScrollIndicatorInsets
      // As the unwrapped view, which can't bounce.
      bounces={false}
      data={rows}
      keyExtractor={(_, index) => String(index)}
      renderItem={({ item }) => (
        <WrappedLine row={item} palette={palette} signs={signs} />
      )}
      initialNumToRender={60}
      maxToRenderPerBatch={60}
      windowSize={12}
      contentContainerStyle={styles.content}
      ListFooterComponent={footer}
    />
  );
}

function PannedList({
  rows,
  palette,
  signs,
  footer,
  resetKey,
}: ListProps & { resetKey: string }) {
  const styles = useStyles();
  const screen = useWindowDimensions().width;
  const gutterWidth = NUMBER_WIDTH + (signs ? SIGN_WIDTH : 0);
  // The code's width at its longest line, so every line scrolls as one page.
  const codeWidth = useMemo(
    () =>
      Math.max(
        screen - gutterWidth,
        24 +
          CHAR_WIDTH *
            Math.min(
              MAX_COLUMNS,
              rows.reduce(
                (widest, row) =>
                  Math.max(widest, row.hunk ? 0 : row.text.length),
                0,
              ),
            ),
      ),
    [rows, screen, gutterWidth],
  );
  // Where each row starts, hunk headers being a little taller.
  const offsets = useMemo(() => {
    const out = [0];
    for (const row of rows)
      out.push(out[out.length - 1] + (row.hunk ? HUNK_HEIGHT : ROW_HEIGHT));
    return out;
  }, [rows]);
  const layout = (_: unknown, index: number) => ({
    length: offsets[index + 1] - offsets[index],
    offset: offsets[index],
    index,
  });

  // Either column scrolls down, and the other follows on the UI thread, frame
  // for frame. The one last touched leads, so the two never chase each other.
  const gutter = useAnimatedRef<FlatList<CodeRow>>();
  const code = useAnimatedRef<FlatList<CodeRow>>();
  const leader = useSharedValue<"code" | "gutter">("code");
  const onCodeScroll = useAnimatedScrollHandler({
    onBeginDrag: () => {
      leader.set("code");
    },
    onScroll: (event) => {
      if (leader.get() === "code")
        scrollTo(gutter, 0, event.contentOffset.y, false);
    },
  });
  const onGutterScroll = useAnimatedScrollHandler({
    onBeginDrag: () => {
      leader.set("gutter");
    },
    onScroll: (event) => {
      if (leader.get() === "gutter")
        scrollTo(code, 0, event.contentOffset.y, false);
    },
  });

  const sideways = useRef<ScrollView>(null);
  const [height, setHeight] = useState(0);
  useEffect(() => {
    sideways.current?.scrollTo({ x: 0, animated: false });
  }, [resetKey]);

  // Both columns pad by the header and home indicator themselves: iOS would
  // inset only one of them on its own, and the numbers would drift off their lines.
  const headerHeight = useHeaderHeight();
  const bottom = useSafeAreaInsets().bottom;
  const shared = {
    contentInsetAdjustmentBehavior: "never" as const,
    // No bounce at the top and bottom: the two columns would stretch apart.
    bounces: false,
    scrollIndicatorInsets: { top: headerHeight, bottom },
    data: rows,
    keyExtractor: (_: CodeRow, index: number) => String(index),
    getItemLayout: layout,
    initialNumToRender: 60,
    maxToRenderPerBatch: 60,
    windowSize: 12,
    contentContainerStyle: [
      styles.content,
      { paddingTop: headerHeight, paddingBottom: 40 + bottom },
    ],
  };

  return (
    // Every box placed and sized outright, no flex: the numbers at the left,
    // the code's sideways scroll from their edge to the screen's, and inside it
    // the code exactly its own width and the view's height, starting at the left.
    <View
      style={styles.split}
      onLayout={(event) => setHeight(event.nativeEvent.layout.height)}
    >
      <Animated.FlatList
        {...shared}
        ref={gutter}
        style={[styles.gutterColumn, { width: gutterWidth }]}
        showsVerticalScrollIndicator={false}
        onScroll={onGutterScroll}
        scrollEventThrottle={16}
        renderItem={({ item }) => <GutterCell row={item} signs={signs} />}
        // The same height as the code's footer, so both lists end together.
        ListFooterComponent={
          footer ? <View style={styles.hidden}>{footer}</View> : null
        }
      />
      <ScrollView
        ref={sideways}
        horizontal
        style={[styles.codeColumn, { left: gutterWidth }]}
        contentOffset={{ x: 0, y: 0 }}
        showsHorizontalScrollIndicator={false}
        // Stops at the first and last column, as the vertical scroll does.
        bounces={false}
        // A drag that starts sideways stays sideways, and one down stays down.
        directionalLockEnabled
        contentInsetAdjustmentBehavior="never"
        contentContainerStyle={{ width: codeWidth, height }}
      >
        <Animated.FlatList
          {...shared}
          ref={code}
          style={{ width: codeWidth, height }}
          onScroll={onCodeScroll}
          scrollEventThrottle={16}
          renderItem={({ item }) => <CodeCell row={item} palette={palette} />}
          ListFooterComponent={footer}
        />
      </ScrollView>
    </View>
  );
}

const GutterCell = memo(function GutterCell({
  row,
  signs,
}: {
  row: CodeRow;
  signs: boolean;
}) {
  const styles = useStyles();
  if (row.hunk) return <View style={[styles.hunk, styles.hunkGutter]} />;
  const tone = row.tone;
  return (
    <View
      style={[
        styles.line,
        styles.gutter,
        tone && { backgroundColor: tone.gutter },
      ]}
    >
      <Text style={[styles.number, tone && { color: tone.fg }]}>
        {row.number ?? ""}
      </Text>
      {signs ? (
        <Text style={[styles.sign, tone && { color: tone.fg }]}>
          {row.sign}
        </Text>
      ) : null}
    </View>
  );
});

const CodeCell = memo(function CodeCell({
  row,
  palette,
}: {
  row: CodeRow;
  palette?: string[];
}) {
  const styles = useStyles();
  if (row.hunk) {
    return (
      <View style={styles.hunk}>
        <Text style={styles.hunkText} numberOfLines={1}>
          {row.text}
        </Text>
      </View>
    );
  }
  return (
    <View style={[styles.line, row.tone && { backgroundColor: row.tone.row }]}>
      <Text style={[styles.code, row.dim && styles.dim]} numberOfLines={1}>
        <Runs row={row} palette={palette} />
      </Text>
    </View>
  );
});

const WrappedLine = memo(function WrappedLine({
  row,
  palette,
  signs,
}: {
  row: CodeRow;
  palette?: string[];
  signs: boolean;
}) {
  const styles = useStyles();
  if (row.hunk) {
    return (
      <View style={styles.hunk}>
        <Text style={styles.hunkText} numberOfLines={1}>
          {row.text}
        </Text>
      </View>
    );
  }
  const tone = row.tone;
  return (
    <View style={[styles.wrappedLine, tone && { backgroundColor: tone.row }]}>
      <View style={[styles.gutter, tone && { backgroundColor: tone.gutter }]}>
        <Text style={[styles.number, tone && { color: tone.fg }]}>
          {row.number ?? ""}
        </Text>
        {signs ? (
          <Text style={[styles.sign, tone && { color: tone.fg }]}>
            {row.sign}
          </Text>
        ) : null}
      </View>
      <Text style={[styles.code, styles.wrappedCode, row.dim && styles.dim]}>
        <Runs row={row} palette={palette} />
      </Text>
    </View>
  );
});

/** A line's text in its syntax colours, or plain without them. */
function Runs({ row, palette }: { row: CodeRow; palette?: string[] }) {
  if (!row.runs || !palette || row.runs.length === 0)
    return <>{row.text || " "}</>;
  return (
    <>
      {row.runs.map(([text, color], index) => (
        <Text
          key={index}
          style={palette[color] ? { color: palette[color] } : undefined}
        >
          {text}
        </Text>
      ))}
    </>
  );
}

const useStyles = themedStyles((colors) => ({
  split: {
    flex: 1,
  },
  gutterColumn: {
    position: "absolute",
    top: 0,
    bottom: 0,
    left: 0,
  },
  codeColumn: {
    position: "absolute",
    top: 0,
    bottom: 0,
    right: 0,
  },
  list: {
    flex: 1,
  },
  content: {
    paddingBottom: 40,
  },
  hidden: {
    opacity: 0,
  },
  line: {
    flexDirection: "row",
    height: ROW_HEIGHT,
  },
  wrappedLine: {
    flexDirection: "row",
    minHeight: ROW_HEIGHT,
  },
  gutter: {
    flexDirection: "row",
    alignSelf: "stretch",
  },
  number: {
    width: NUMBER_WIDTH,
    paddingRight: 8,
    textAlign: "right",
    color: withAlpha(colors.content, 0.35),
    fontFamily: MONO,
    fontSize: 11,
    lineHeight: ROW_HEIGHT,
  },
  sign: {
    width: SIGN_WIDTH,
    textAlign: "center",
    color: "transparent",
    fontFamily: MONO,
    fontSize: 12,
    lineHeight: ROW_HEIGHT,
  },
  code: {
    paddingLeft: 4,
    paddingRight: 20,
    color: withAlpha(colors.content, 0.8),
    fontFamily: MONO,
    fontSize: 12,
    lineHeight: ROW_HEIGHT,
  },
  wrappedCode: {
    flex: 1,
  },
  dim: {
    opacity: 0.7,
  },
  hunk: {
    height: HUNK_HEIGHT,
    justifyContent: "center",
    paddingHorizontal: 12,
    backgroundColor: withAlpha(colors.content, 0.05),
  },
  hunkGutter: {
    paddingHorizontal: 0,
  },
  hunkText: {
    color: withAlpha(colors.content, 0.4),
    fontFamily: MONO,
    fontSize: 11,
  },
}));

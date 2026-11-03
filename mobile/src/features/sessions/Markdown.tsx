import {
  createContext,
  Fragment,
  memo,
  use,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { Linking, Platform, ScrollView, StyleSheet, Text, View } from "react-native";

import { FileTypeIcon } from "@/features/files/FileTypeIcon";
import { withAlpha, themedStyles, useColors } from "@/ui/theme";

import {
  numberWords,
  parseMarkdown,
  splitWords,
  type Inline,
  type MarkdownBlock,
} from "./markdownParser";
import { closeOpenMarkers, useStreamedText, useWordFade, type WordClock } from "./wordReveal";

const MONO = Platform.select({ ios: "Menlo", default: "monospace" });

/** Prose inside a fold: the agent talking while it works, one notch quieter. */
const Muted = createContext(false);

/**
 * Opens a file a transcript names, as the desktop's FileOpenContext does.
 * Unset where there is no project to read it from, and files show unlinked.
 */
export const FileOpener = createContext<((path: string) => void) | undefined>(undefined);

/** Words still fading in, and the clock they fade by. */
type Fade = { clock: WordClock; now: number; ms: number; from: number };

/**
 * An agent reply, read as Markdown like the desktop's AgentMarkdown. While it
 * streams, text shows as it arrives, a word still being written waits until
 * it is whole, and each new word fades in over a time that follows the
 * stream's pace, as the desktop's word fade does.
 */
export const Markdown = memo(function Markdown({
  text,
  muted = false,
  streaming = false,
  animate = false,
}: {
  text: string;
  /** As the desktop's `zen-fold-prose`: muted ink, headings as bold lines. */
  muted?: boolean;
  streaming?: boolean;
  /** Fade in the words there on mount too: a reply that arrives whole. */
  animate?: boolean;
}) {
  const styles = useStyles();
  const shown = useStreamedText(text, streaming);
  const { blocks, count, ranges } = useMemo(() => {
    const blocks = parseMarkdown(streaming ? closeOpenMarkers(shown) : shown);
    return { blocks, ...numberWords(blocks) };
  }, [shown, streaming]);
  const { clock, now, fadeMs } = useWordFade(count, animate || streaming);
  const from = clock.firstFading(now, fadeMs);
  const fade: Fade = { clock, now, ms: fadeMs, from };
  return (
    <Muted value={muted}>
      <View style={styles.blocks}>
        {blocks.map((block, index) => (
          // Blocks with nothing fading keep their props, so they skip the frame.
          <BlockView key={index} block={block} fade={ranges[index][1] > from ? fade : undefined} />
        ))}
      </View>
    </Muted>
  );
});

function Blocks({ blocks, fade }: { blocks: MarkdownBlock[]; fade?: Fade }) {
  const styles = useStyles();
  return (
    <View style={styles.blocks}>
      {blocks.map((block, index) => (
        <BlockView key={index} block={block} fade={fade} />
      ))}
    </View>
  );
}

/** Each parsed block's contents as text, to tell a re-parsed block from a changed one. */
const signatures = new WeakMap<MarkdownBlock, string>();
function signature(block: MarkdownBlock) {
  let value = signatures.get(block);
  if (value === undefined) {
    value = JSON.stringify(block);
    signatures.set(block, value);
  }
  return value;
}

/** A streaming reply is parsed anew with every burst; the blocks before the
 *  one being written come out the same and skip the render. */
function sameBlock(
  previous: { block: MarkdownBlock; fade?: Fade },
  next: { block: MarkdownBlock; fade?: Fade },
) {
  return (
    previous.fade === next.fade &&
    (previous.block === next.block || signature(previous.block) === signature(next.block))
  );
}

const BlockView = memo(function BlockView({ block, fade }: { block: MarkdownBlock; fade?: Fade }) {
  const styles = useStyles();
  const muted = use(Muted);
  switch (block.type) {
    case "paragraph":
      return (
        <Text style={[styles.text, muted && styles.mutedText]} selectable>
          <Inlines nodes={block.inline} fade={fade} />
        </Text>
      );
    case "heading":
      return (
        <Text
          style={[
            styles.text,
            styles.heading,
            muted ? styles.mutedHeading : block.level <= 2 && styles.headingLarge,
          ]}
          selectable
        >
          <Inlines nodes={block.inline} fade={fade} />
        </Text>
      );
    case "code":
      return (
        <ScrollView
          horizontal
          showsHorizontalScrollIndicator={false}
          style={styles.codeBlock}
          contentContainerStyle={styles.codeContent}
        >
          <Text style={styles.codeText} selectable>
            {block.text}
          </Text>
        </ScrollView>
      );
    case "list":
      return (
        <View style={styles.list}>
          {block.items.map((item, index) => (
            <View key={index} style={styles.item}>
              <Text style={[styles.text, styles.marker]}>
                {block.ordered ? `${block.start + index}.` : "•"}
              </Text>
              <View style={styles.itemBody}>
                <Blocks blocks={item} fade={fade} />
              </View>
            </View>
          ))}
        </View>
      );
    case "quote":
      return (
        <View style={styles.quote}>
          <Blocks blocks={block.children} fade={fade} />
        </View>
      );
    case "table":
      return <Table rows={block.rows} fade={fade} />;
    case "rule":
      return <View style={styles.rule} />;
  }
}, sameBlock);

/** Columns share the screen's width, down to a floor; past it, the table scrolls. */
const MIN_COLUMN_WIDTH = 140;

function Table({ rows, fade }: { rows: Inline[][][]; fade?: Fade }) {
  const styles = useStyles();
  const [width, setWidth] = useState(0);
  const columns = Math.max(1, ...rows.map((row) => row.length));
  const inner = width - StyleSheet.hairlineWidth * 2;
  const cellWidth = Math.max(MIN_COLUMN_WIDTH, Math.floor(inner / columns));
  return (
    <ScrollView
      horizontal
      showsHorizontalScrollIndicator={false}
      onLayout={(event) => setWidth(event.nativeEvent.layout.width)}
    >
      <View style={styles.table}>
        {rows.map((row, rowIndex) => (
          <View key={rowIndex} style={[styles.row, rowIndex > 0 && styles.rowDivider]}>
            {row.map((cell, cellIndex) => (
              <Text
                key={cellIndex}
                style={[styles.cell, { width: cellWidth }, rowIndex === 0 && styles.headerCell]}
                selectable
              >
                <Inlines nodes={cell} fade={fade} />
              </Text>
            ))}
          </View>
        ))}
      </View>
    </ScrollView>
  );
}

function Inlines({ nodes, fade }: { nodes: Inline[]; fade?: Fade }) {
  const styles = useStyles();
  const tone = use(Muted) ? 0.65 : 1;
  const openFile = use(FileOpener);
  return (
    <>
      {nodes.map((node, index) => (
        <Fragment key={index}>{inline(node, fade, tone, styles, openFile)}</Fragment>
      ))}
    </>
  );
}

/**
 * A run of text with its fading words in their own spans. Words already in
 * stay one plain string; a word keeps its span, by its number, until it is.
 */
function FadingText({ text, word, fade, tone }: { text: string; word: number; fade: Fade; tone: number }) {
  const colors = useColors();
  const parts = splitWords(text);
  const out: ReactNode[] = [];
  let plain = "";
  let index = word;
  for (const part of parts) {
    if (/^\s/.test(part) || index < fade.from) {
      plain += part;
      if (!/^\s/.test(part)) index++;
      continue;
    }
    if (plain) out.push(plain);
    plain = "";
    const alpha = fade.clock.alpha(index, fade.now, fade.ms);
    out.push(
      <Text key={index} style={{ color: withAlpha(colors.content, alpha * tone) }}>
        {part}
      </Text>,
    );
    index++;
  }
  if (plain) out.push(plain);
  return <>{out}</>;
}

function inline(
  node: Inline,
  fade: Fade | undefined,
  tone: number,
  styles: ReturnType<typeof useStyles>,
  openFile: ((path: string) => void) | undefined,
) {
  switch (node.type) {
    case "text":
      return fade && node.word !== undefined ? (
        <FadingText text={node.text} word={node.word} fade={fade} tone={tone} />
      ) : (
        node.text
      );
    case "code": {
      const fileName = inlineFileName(node.text);
      if (!fileName) return <Text style={[styles.inlineCode, styles.inlineCodeTint]}>{node.text}</Text>;
      return (
        <Text onPress={openFile ? () => openFile(node.text.trim()) : undefined}>
          {/* A zero-width space: iOS takes a paragraph's line height from its
              first character, and the icon's placeholder carries none. */}
          {"\u200B"}
          <View style={styles.inlineIcon}>
            <FileTypeIcon name={fileName} isDir={false} size={14} />
          </View>
          {/* A word joiner: no line break between the icon and its name. */}
          {"\u2060"}
          <Text style={[styles.inlineCode, openFile && styles.fileRef]}>{node.text}</Text>
        </Text>
      );
    }
    case "strong":
      return (
        <Text style={styles.strong}>
          <Inlines nodes={node.children} fade={fade} />
        </Text>
      );
    case "em":
      return (
        <Text style={styles.em}>
          <Inlines nodes={node.children} fade={fade} />
        </Text>
      );
    case "strike":
      return (
        <Text style={styles.strike}>
          <Inlines nodes={node.children} fade={fade} />
        </Text>
      );
    case "link":
      return (
        <Text
          style={styles.link}
          onPress={
            /^https?:\/\//.test(node.href) ? () => void Linking.openURL(node.href) : undefined
          }
        >
          <Inlines nodes={node.children} fade={fade} />
        </Text>
      );
  }
}

/**
 * The file a code span names, for its icon: the desktop's inlineFileName
 * (src/features/sessions/ui/AgentMarkdown.tsx). One token, a `:line` or
 * `#L` suffix dropped, ending in a name with a plausible extension.
 */
function inlineFileName(value: string): string | undefined {
  const text = value.trim();
  if (!text || text.length > 240 || /\s/.test(text)) return undefined;
  const fileName = text
    .replace(/(?::\d+(?::\d+)?|#L\d+(?:-L\d+)?)$/, "")
    .split(/[/\\]/)
    .filter(Boolean)
    .pop();
  if (!fileName || !/^[\w%@+().-]+$/.test(fileName)) return undefined;
  if (/^(dockerfile|makefile|gemfile|license)$/i.test(fileName)) return fileName;
  const extension = fileName.includes(".") ? fileName.split(".").pop() : undefined;
  return extension && /^[a-z][a-z0-9+-]{0,11}$/i.test(extension) ? fileName : undefined;
}

const useStyles = themedStyles((colors) => ({
  blocks: {
    gap: 12,
  },
  text: {
    color: colors.content,
    fontSize: 16,
    lineHeight: 24,
  },
  mutedText: {
    color: withAlpha(colors.content, 0.65),
  },
  mutedHeading: {
    fontSize: 14,
    color: withAlpha(colors.content, 0.8),
  },
  heading: {
    fontWeight: "600",
    marginTop: 4,
  },
  headingLarge: {
    fontSize: 18,
    lineHeight: 26,
  },
  strong: {
    fontWeight: "600",
  },
  em: {
    fontStyle: "italic",
  },
  strike: {
    textDecorationLine: "line-through",
    color: colors.muted,
  },
  link: {
    color: colors.accent,
  },
  inlineCode: {
    fontFamily: MONO,
    fontSize: 15,
  },
  fileRef: {
    color: colors.fileRef,
  },
  inlineCodeTint: {
    backgroundColor: withAlpha(colors.content, 0.07),
  },
  inlineIcon: {
    marginRight: 4,
    // Measured on device: the placeholder sits ~1pt above the name's caps.
    transform: [{ translateY: 1 }],
  },
  codeBlock: {
    borderRadius: 10,
    borderCurve: "continuous",
    backgroundColor: withAlpha(colors.content, 0.05),
  },
  codeContent: {
    padding: 12,
  },
  codeText: {
    color: withAlpha(colors.content, 0.85),
    fontFamily: MONO,
    fontSize: 13,
    lineHeight: 19,
  },
  list: {
    gap: 6,
  },
  item: {
    flexDirection: "row",
    gap: 8,
  },
  marker: {
    minWidth: 14,
    color: colors.muted,
  },
  itemBody: {
    flex: 1,
    minWidth: 0,
  },
  quote: {
    borderLeftWidth: 2,
    borderLeftColor: withAlpha(colors.content, 0.2),
    paddingLeft: 12,
  },
  table: {
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: colors.stroke,
    borderRadius: 8,
  },
  row: {
    flexDirection: "row",
  },
  rowDivider: {
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: colors.stroke,
  },
  cell: {
    paddingHorizontal: 10,
    paddingVertical: 6,
    color: colors.content,
    fontSize: 14,
    lineHeight: 20,
  },
  headerCell: {
    fontWeight: "600",
  },
  rule: {
    height: StyleSheet.hairlineWidth,
    backgroundColor: colors.stroke,
  },
}));

import {
  createContext,
  memo,
  use,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import * as Clipboard from "expo-clipboard";
import { Linking, Platform, Pressable, ScrollView, StyleSheet, Text, View } from "react-native";

import { FileTypeIcon } from "@/features/files/FileTypeIcon";
import { Icon } from "@/ui/Icon";
import { withAlpha, themedStyles, useColors } from "@/ui/theme";

import {
  holdsImage,
  numberWords,
  parseMarkdown,
  splitImages,
  splitWords,
  type Inline,
  type MarkdownBlock,
  type MarkdownImage,
} from "./markdownParser";
import { closeOpenMarkers, useStreamedText, useWordFade, type WordClock } from "./wordReveal";
import { codeFenceHeader } from "./codeFence";

const MONO = Platform.select({ ios: "Menlo", default: "monospace" });

/** Prose inside a fold: the agent talking while it works, one notch quieter. */
const Muted = createContext(false);

/**
 * Opens a file a transcript names, as the desktop's FileOpenContext does.
 * Unset where there is no project to read it from, and files show unlinked.
 */
export const FileOpener = createContext<((path: string) => void) | undefined>(undefined);

/** Draws an image; `href` is the link around it, if any. */
export type RenderImage = (image: MarkdownImage, href?: string) => ReactNode;

const ImageRenderer = createContext<RenderImage | undefined>(undefined);

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
  renderImage,
}: {
  text: string;
  /** As the desktop's `zen-fold-prose`: muted ink, headings as bold lines. */
  muted?: boolean;
  streaming?: boolean;
  /** Fade in the words there on mount too: a reply that arrives whole. */
  animate?: boolean;
  /** A provider's text, which mixes HTML in, read with its images drawn by this. */
  renderImage?: RenderImage;
}) {
  const styles = useStyles();
  const shown = useStreamedText(text, streaming);
  const html = renderImage !== undefined;
  const { blocks, count, ranges } = useMemo(() => {
    const blocks = parseMarkdown(streaming ? closeOpenMarkers(shown) : shown, { html });
    return { blocks, ...numberWords(blocks) };
  }, [shown, streaming, html]);
  const { clock, now, fadeMs } = useWordFade(count, animate || streaming);
  const from = clock.firstFading(now, fadeMs);
  const fade: Fade = { clock, now, ms: fadeMs, from };
  return (
    <Muted value={muted}>
      <ImageRenderer value={renderImage}>
        <View style={styles.blocks}>
          {blocks.map((block, index) => (
            // Blocks with nothing fading keep their props, so they skip the frame.
            <BlockView key={index} block={block} fade={ranges[index][1] > from ? fade : undefined} />
          ))}
        </View>
      </ImageRenderer>
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
      return <CodeBlock text={block.text} language={block.lang} />;
    case "list":
      return (
        <View style={styles.list}>
          {block.items.map((item, index) => (
            <View key={index} style={styles.item}>
              {block.tasks?.[index] !== undefined ? (
                <TaskBox done={block.tasks[index]} />
              ) : (
                <Text style={[styles.text, styles.marker]}>
                  {block.ordered ? `${block.start + index}.` : "•"}
                </Text>
              )}
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
    case "image":
      return <ImageBlock image={block.image} href={block.href} />;
    case "details":
      return <Details block={block} fade={fade} />;
  }
}, sameBlock);

function ImageBlock({ image, href }: { image: MarkdownImage; href?: string }) {
  const renderImage = use(ImageRenderer);
  return <>{renderImage?.(image, href)}</>;
}

function TaskBox({ done }: { done: boolean }) {
  const colors = useColors();
  const styles = useStyles();
  return (
    <View
      accessible
      accessibilityRole="checkbox"
      accessibilityState={{ checked: done }}
      style={[styles.task, done && { backgroundColor: colors.accent, borderColor: colors.accent }]}
    >
      {done ? <Icon name="check" size={12} color={colors.inverse} /> : null}
    </View>
  );
}

/** A `<details>` fold, shut until pressed unless the text says `open`. */
function Details({
  block,
  fade,
}: {
  block: Extract<MarkdownBlock, { type: "details" }>;
  fade?: Fade;
}) {
  const colors = useColors();
  const styles = useStyles();
  const [open, setOpen] = useState(block.open);
  return (
    <View style={styles.details}>
      <Pressable
        accessibilityRole="button"
        accessibilityState={{ expanded: open }}
        onPress={() => setOpen((value) => !value)}
        style={styles.summary}
      >
        <View style={open && styles.chevronOpen}>
          <Icon name="chevronRight" size={14} color={colors.muted} />
        </View>
        <Text style={[styles.text, styles.summaryText]}>
          <Inlines nodes={block.summary} fade={fade} />
        </Text>
      </Pressable>
      {open ? <Blocks blocks={block.children} fade={fade} /> : null}
    </View>
  );
}

function CodeBlock({ text, language }: { text: string; language: string }) {
  const styles = useStyles();
  const colors = useColors();
  const header = codeFenceHeader(language);
  const body = useMemo(() => ({ text }), [text]);
  const [copiedBody, setCopiedBody] = useState<typeof body>();
  const copied = copiedBody === body;
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const copyAttempt = useRef({ id: 0 });
  useEffect(() => {
    const attempt = copyAttempt.current;
    return () => {
      attempt.id++;
      clearTimeout(timer.current);
    };
  }, [text]);
  return (
    <View style={styles.codeBlock}>
      <View style={styles.codeHeader}>
        <View style={styles.codeLanguage}>
          <FileTypeIcon name={header.fileName} isDir={false} size={16} />
          <Text style={styles.codeLabel} numberOfLines={1}>{header.label}</Text>
        </View>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={copied ? "Copied" : "Copy code"}
          style={({ pressed }) => [styles.codeCopy, pressed && styles.codeCopyPressed]}
          onPress={async () => {
            const attempt = ++copyAttempt.current.id;
            clearTimeout(timer.current);
            setCopiedBody(undefined);
            try {
              const saved = await Clipboard.setStringAsync(text);
              if (!saved || attempt !== copyAttempt.current.id) return;
              setCopiedBody(body);
              clearTimeout(timer.current);
              timer.current = setTimeout(() => setCopiedBody(undefined), 2000);
            } catch {
              // Keep the copy action available if the clipboard is unavailable.
            }
          }}
        >
          <Icon name={copied ? "check" : "copy"} size={16} color={colors.muted} />
        </Pressable>
      </View>
      <ScrollView horizontal showsHorizontalScrollIndicator={false} contentContainerStyle={styles.codeContent}>
        <Text style={styles.codeText} selectable>{text}</Text>
      </ScrollView>
    </View>
  );
}

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
              <Cell
                key={cellIndex}
                nodes={cell}
                width={cellWidth}
                header={rowIndex === 0}
                fade={fade}
              />
            ))}
          </View>
        ))}
      </View>
    </ScrollView>
  );
}

function Cell({
  nodes,
  width,
  header,
  fade,
}: {
  nodes: Inline[];
  width: number;
  header: boolean;
  fade?: Fade;
}) {
  const styles = useStyles();
  const text = [styles.cell, header && styles.headerCell];
  if (!nodes.some(holdsImage)) {
    return (
      <Text style={[text, { width }]} selectable>
        <Inlines nodes={nodes} fade={fade} />
      </Text>
    );
  }
  // Before and after, side by side: the table many pull requests are written as.
  return (
    <View style={[styles.imageCell, { width }]}>
      {splitImages(nodes).map((piece, index) =>
        piece.kind === "image" ? (
          <ImageBlock key={index} image={piece.image} href={piece.href} />
        ) : (
          <Text key={index} style={[text, styles.imageCellText]} selectable>
            <Inlines nodes={piece.inline} fade={fade} />
          </Text>
        ),
      )}
    </View>
  );
}

/** The word number just past the last word of `node`, to tell if any of it still fades. */
const ends = new WeakMap<Inline, number>();
function inlineEnd(node: Inline): number {
  let end = ends.get(node);
  if (end === undefined) {
    if (node.type === "text")
      end = (node.word ?? 0) + splitWords(node.text).filter((part) => !/^\s/.test(part)).length;
    else if (node.type === "strong" || node.type === "em" || node.type === "strike")
      end = Math.max(0, ...node.children.map(inlineEnd));
    else end = 0;
    ends.set(node, end);
  }
  return end;
}

const nodeSignatures = new WeakMap<Inline, string>();
function sameInline(
  previous: { node: Inline; fade?: Fade },
  next: { node: Inline; fade?: Fade },
) {
  if (previous.fade !== next.fade) return false;
  if (previous.node === next.node) return true;
  let a = nodeSignatures.get(previous.node);
  if (a === undefined) nodeSignatures.set(previous.node, (a = JSON.stringify(previous.node)));
  let b = nodeSignatures.get(next.node);
  if (b === undefined) nodeSignatures.set(next.node, (b = JSON.stringify(next.node)));
  return a === b;
}

function Inlines({ nodes, fade }: { nodes: Inline[]; fade?: Fade }) {
  return (
    <>
      {nodes.map((node, index) => (
        // A node whose words have all settled gets no fade, so it keeps its
        // props and skips the frame; only the node still fading renders.
        <InlineNode key={index} node={node} fade={fade && inlineEnd(node) > fade.from ? fade : undefined} />
      ))}
    </>
  );
}

const InlineNode = memo(function InlineNode({ node, fade }: { node: Inline; fade?: Fade }) {
  const styles = useStyles();
  const tone = use(Muted) ? 0.65 : 1;
  const openFile = use(FileOpener);
  return <>{inline(node, fade, tone, styles, openFile)}</>;
}, sameInline);

/** `withAlpha` strings by ink and alpha: the words of a burst share one alpha, so most are looked up. */
const alphaColors = new Map<string, Map<number, string>>();
function inkAt(ink: string, alpha: number): string {
  let byAlpha = alphaColors.get(ink);
  if (!byAlpha) alphaColors.set(ink, (byAlpha = new Map()));
  let color = byAlpha.get(alpha);
  if (color === undefined) {
    // A fade passes through many alphas; keep only a few seconds' worth.
    if (byAlpha.size >= 512) byAlpha.clear();
    byAlpha.set(alpha, (color = withAlpha(ink, alpha)));
  }
  return color;
}

/** One fading word. Its colour is its only prop, so it renders only when that changes. */
const FadingWord = memo(function FadingWord({ text, color }: { text: string; color: string }) {
  return <Text style={{ color }}>{text}</Text>;
});

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
    out.push(<FadingWord key={index} text={part} color={inkAt(colors.content, alpha * tone)} />);
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
    // Images stand apart from text; one left in a heading shows what it is of.
    case "image":
      return node.alt;
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
    paddingTop: 0,
  },
  codeHeader: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
  },
  codeLanguage: {
    flex: 1,
    minWidth: 0,
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
    paddingLeft: 12,
  },
  codeLabel: {
    flexShrink: 1,
    color: colors.muted,
    fontSize: 12,
  },
  codeCopy: {
    width: 44,
    height: 44,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: 10,
  },
  codeCopyPressed: {
    backgroundColor: withAlpha(colors.content, 0.08),
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
  task: {
    width: 18,
    height: 18,
    marginTop: 3,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: 5,
    borderCurve: "continuous",
    borderWidth: 1.5,
    borderColor: colors.muted,
  },
  details: {
    gap: 10,
  },
  summary: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    minHeight: 32,
  },
  summaryText: {
    flex: 1,
    fontWeight: "600",
  },
  chevronOpen: {
    transform: [{ rotate: "90deg" }],
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
  imageCell: {
    gap: 6,
    padding: 6,
  },
  imageCellText: {
    paddingHorizontal: 4,
    paddingVertical: 0,
  },
  rule: {
    height: StyleSheet.hairlineWidth,
    backgroundColor: colors.stroke,
  },
}));

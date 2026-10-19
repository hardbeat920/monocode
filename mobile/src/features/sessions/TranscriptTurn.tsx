import * as Clipboard from "expo-clipboard";
import * as Haptics from "expo-haptics";
import { createContext, memo, use, useEffect, useRef, useState } from "react";
import { Pressable, StyleSheet, Text, View } from "react-native";
import Svg, { Path } from "react-native-svg";

import { PixelMascot } from "@/features/mascots/PixelMascot";
import { useCollapsedToolCalls } from "@/features/settings/toolCalls";
import type {
  Turn,
  TurnItem,
  WorkKind,
  WorkPhase,
} from "@/features/workspace/types";
import { Collapsible } from "@/ui/Collapsible";
import { Icon, type IconName } from "@/ui/Icon";
import { Shimmer } from "@/ui/Shimmer";
import { withAlpha, themedStyles, useColors } from "@/ui/theme";

import { Markdown } from "./Markdown";
import { ProviderLogo } from "./ProviderLogo";
import { sessionErrorContent, type SessionErrorNotice } from "./sessionError";
import {
  dayStamp,
  formatClockTime,
  formatWorkingDuration,
  hasTurnMetrics,
} from "./transcriptFormat";
import { TurnMetricsSheet } from "./TurnMetricsSheet";

/** Who a Mono's turns are by, in place of the model and provider mark. */
export type TranscriptAgent = { name: string; mascot: string; color: string };

type HeaderItem = Extract<TurnItem, { type: "header" }>;

const GUTTER = 20;
/** One size for the work, fold line to steps, matching the reply text. */
const WORK_TEXT = 16;
const WORK_LINE = 24;
/** The fold line's mark, sized to its label. */
const FOLD_MARK = 16;
/** A phase's icon, the same size, so its steps' rail runs under its centre. */
const PHASE_ICON = 16;
/** Where a rail's branch starts, so its curve lands on the centre of a row's
 *  first line: 4 of row padding plus half a line, less the 8px curve. */
const BRANCH_TOP = 4 + WORK_LINE / 2 - 8;
/** Where the fold's rail runs: the centre of the mark on the fold line. */
const RAIL_X = GUTTER + FOLD_MARK / 2;
/** Where a row's content starts, level with the fold line's label. */
const CONTENT_X = GUTTER + FOLD_MARK + 6;
/** The desktop's `text-content/45` icons. */
/** The desktop footer's `text-content/40` buttons. */
/** The desktop's rail: content at 14% over the background, opaque so lines meet cleanly. */
/** The desktop's `bg-content/10` over the background, opaque so the tail can overlap. */
/** The desktop's `bg-content/[0.07]` pills: day stamps and a Mono's signature. */
// The desktop's bubble tail (src/styles/index.css), drawn against an 18px corner.
const TAIL =
  "M9 15.6C9.05 16.3 9.05 17.4 9.05 18.6C9.05 20 8.3 21.4 7.4 22.6Q6.9 23.4 7.8 23.5C9.6 23.3 11.6 22.3 13.2 21.5C15 20.5 16.2 19.6 17.2 18.8C17.9 18.25 18.6 18 19.6 18L18 18A18 18 0 0 1 9 15.6Z";

/**
 * One turn, laid out as AgentTranscript does: the prompt in a bubble on the
 * right, the work folded behind one status line, the answer as full-width
 * Markdown, and when it landed underneath. A Mono's turn signs with its mascot
 * and name in a pill and keeps its work in that line.
 */
export const TranscriptTurn = memo(function TranscriptTurn({
  turn,
  agent,
  onError,
}: {
  turn: Turn;
  agent?: TranscriptAgent;
  onError: (notice: SessionErrorNotice) => void;
}) {
  const styles = useStyles();
  return (
    <View>
      {turn.stampAt != null ? <DaySeparator at={turn.stampAt} /> : null}
      {turn.items.map((item, index) => {
        const previous = turn.items[index - 1];
        switch (item.type) {
          case "user":
            return <UserMessage key={item.id} item={item} tail={!!agent} />;
          case "header":
            return agent ? (
              <MonoHeader key={item.id} item={item} agent={agent} />
            ) : (
              <FoldLine key={item.id} item={item} />
            );
          case "work":
            return (
              <View key={item.id} style={styles.work}>
                <WorkPhases phases={item.phases} live={item.live} />
              </View>
            );
          case "text":
            return (
              <View
                key={item.id}
                style={[
                  styles.reply,
                  previous && previous.type !== "user" && previous.type !== "text"
                    ? styles.replyUnderLine
                    : null,
                ]}
              >
                <Reply text={item.text} streaming={!!item.streaming} />
              </View>
            );
          case "notice":
            return item.error ? (
              <ErrorNotice key={item.id} item={item} onPress={() => onError(item)} />
            ) : (
              <Text key={item.id} style={styles.notice}>
                {item.text}
              </Text>
            );
        }
      })}
      {turn.footer ? <TurnFooter turn={turn} /> : null}
    </View>
  );
});

function ErrorNotice({ item, onPress }: { item: SessionErrorNotice; onPress: () => void }) {
  const colors = useColors();
  const styles = useStyles();
  const { title, recovery } = sessionErrorContent(item.text);
  return (
    <Pressable
      onPress={onPress}
      accessibilityRole="button"
      accessibilityLabel={`${title}. View details`}
      style={({ pressed }) => [styles.errorNotice, pressed && styles.pressed]}
    >
      <Icon name={recovery ? "lock" : "chat"} size={18} color={colors.muted} />
      <View style={styles.errorNoticeText}>
        <Text style={styles.errorNoticeTitle}>{title}</Text>
        <Text style={styles.errorNoticeHint}>View details</Text>
      </View>
      <Icon name="chevronRight" size={16} color={colors.faint} />
    </Pressable>
  );
}

/**
 * What a finished turn leaves under the answer, as on desktop: copy the reply,
 * its token metrics, and when it landed. Handoff and second opinion stay on
 * the desktop.
 */
function TurnFooter({ turn }: { turn: Turn }) {
  const colors = useColors();
  const styles = useStyles();
  const [showMetrics, setShowMetrics] = useState(false);
  // Mounted on first open and kept, so the sheet can animate closed.
  const [metricsOpened, setMetricsOpened] = useState(false);
  const metrics = hasTurnMetrics(turn.metrics) ? turn.metrics : undefined;
  return (
    <View style={styles.footer}>
      {metricsOpened && metrics ? (
        <TurnMetricsSheet
          isPresented={showMetrics}
          onDismiss={() => setShowMetrics(false)}
          metrics={metrics}
          elapsedMs={turn.durationMs ?? null}
          completedAt={turn.completedAt}
        />
      ) : null}
      <View style={styles.footerActions}>
        {turn.copyText ? <CopyButton text={turn.copyText} /> : null}
        {metrics ? (
          <Pressable
            onPress={() => {
              setMetricsOpened(true);
              setShowMetrics(true);
            }}
            accessibilityRole="button"
            accessibilityLabel="Turn metrics"
            hitSlop={6}
            style={({ pressed }) => [
              styles.footerButton,
              (pressed || showMetrics) && styles.footerButtonActive,
            ]}
          >
            <Icon name="metrics" size={16} color={withAlpha(colors.content, 0.4)} />
          </Pressable>
        ) : null}
      </View>
      {turn.completedAt != null ? (
        <>
          {turn.copyText || metrics ? <View style={styles.footerDot} /> : null}
          <Text style={styles.footerTime}>{formatClockTime(turn.completedAt)}</Text>
        </>
      ) : null}
    </View>
  );
}

function CopyButton({ text }: { text: string }) {
  const colors = useColors();
  const styles = useStyles();
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  return (
    <Pressable
      onPress={async () => {
        try {
          await Clipboard.setStringAsync(text);
        } catch {
          return;
        }
        void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light);
        setCopied(true);
        clearTimeout(timer.current);
        timer.current = setTimeout(() => setCopied(false), 2000);
      }}
      accessibilityRole="button"
      accessibilityLabel={copied ? "Copied" : "Copy response"}
      hitSlop={6}
      style={({ pressed }) => [styles.footerButton, pressed && styles.footerButtonActive]}
    >
      <Icon name={copied ? "check" : "copy"} size={16} color={withAlpha(colors.content, 0.4)} />
    </Pressable>
  );
}

/**
 * True once the transcript has shown what was there when it opened. A reply
 * that mounts after that is new and fades in, as the desktop's revealOnMount.
 */
export const TranscriptOpened = createContext(false);

function Reply({ text, streaming }: { text: string; streaming: boolean }) {
  const opened = use(TranscriptOpened);
  const [animate] = useState(opened);
  return <Markdown text={text} streaming={streaming} animate={animate} />;
}

function DaySeparator({ at }: { at: number }) {
  const styles = useStyles();
  const [day, time] = dayStamp(at);
  return (
    <View style={styles.daySeparator} accessibilityLabel={`${day} ${time}`}>
      <View style={styles.dayPill}>
        <Text style={styles.dayText}>
          <Text style={styles.dayName}>{day}</Text> {time}
        </Text>
      </View>
    </View>
  );
}

function UserMessage({
  item,
  tail,
}: {
  item: Extract<TurnItem, { type: "user" }>;
  tail: boolean;
}) {
  const styles = useStyles();
  const [singleLine, setSingleLine] = useState(false);
  const [overflows, setOverflows] = useState(false);
  const [expanded, setExpanded] = useState(false);

  // In a Mono's chat a few emoji stand alone, like a reaction: no bubble.
  if (tail && item.emojiOnly) {
    return (
      <View style={[styles.userRow, item.compact && styles.userRowCompact]}>
        <View>
          <Text style={styles.emoji}>{item.text.trim()}</Text>
          {item.reaction ? <Reaction emoji={item.reaction} /> : null}
        </View>
      </View>
    );
  }

  const withTail = tail && !item.compact;
  return (
    <View style={[styles.userRow, item.compact && styles.userRowCompact]}>
      <View
        style={[
          styles.bubble,
          singleLine ? styles.bubbleRound : styles.bubbleSquare,
        ]}
      >
        {item.attachments ? (
          <Text style={styles.attachments}>
            {item.attachments === 1 ? "1 attachment" : `${item.attachments} attachments`}
          </Text>
        ) : null}
        {item.text ? (
          <Text
            style={styles.userText}
            selectable
            numberOfLines={expanded ? undefined : 4}
            onTextLayout={(event) => {
              const lines = event.nativeEvent.lines.length;
              setSingleLine(lines === 1 && !item.attachments);
              if (!expanded) setOverflows(lines >= 4 && isClamped(event.nativeEvent.lines, item.text));
            }}
          >
            {item.text}
          </Text>
        ) : null}
        {overflows ? (
          <Pressable onPress={() => setExpanded((value) => !value)} hitSlop={8}>
            <Text style={styles.showMore}>{expanded ? "Show less" : "Show more"}</Text>
          </Pressable>
        ) : null}
        {withTail ? <BubbleTail radius={singleLine ? 18 : 14} /> : null}
        {item.reaction ? <Reaction emoji={item.reaction} /> : null}
      </View>
      {!item.compact && item.at != null ? (
        <Text style={styles.userTime}>{formatClockTime(item.at)}</Text>
      ) : null}
    </View>
  );
}

/** Whether a 4-line clamp cut the message short. */
function isClamped(lines: { text: string }[], text: string) {
  return lines.map((line) => line.text).join("").length < text.replace(/\n/g, "").length;
}

function BubbleTail({ radius }: { radius: number }) {
  const colors = useColors();
  const styles = useStyles();
  const size = (radius * 4) / 3;
  return (
    <View
      pointerEvents="none"
      style={[
        styles.tail,
        { width: size, height: size, bottom: -radius / 3, transform: [{ scaleX: -1 }] },
      ]}
    >
      <Svg width={size} height={size} viewBox="0 0 24 24">
        <Path d={TAIL} fill={colors.bubble} />
      </Svg>
    </View>
  );
}

/** A Mono's emoji answer, on the corner of the message it answers. */
function Reaction({ emoji }: { emoji: string }) {
  const styles = useStyles();
  return (
    <View style={styles.reaction} accessibilityLabel={`Reacted ${emoji}`}>
      <Text style={styles.reactionText}>{emoji}</Text>
    </View>
  );
}

/**
 * The line a turn's work folds behind: the provider's mark and the model's
 * clock, ticking while it works and how long it took once done. Tapped open,
 * the work hangs off it on a rail: the agent's notes, muted, between the
 * phases of its work, as on desktop.
 */
function FoldLine({ item }: { item: HeaderItem }) {
  const colors = useColors();
  const styles = useStyles();
  const [open, setOpen] = useState(false);
  const elapsed = useElapsed(item.live ? item.startedAt : undefined);
  const label =
    item.label ??
    (item.live
      ? (item.waiting ?? formatWorkingDuration(elapsed, item.model))
      : formatWorkingDuration(item.durationMs ?? null, item.model, true));
  const expandable = item.entries.length > 0;
  const shown = open && expandable;
  const mark = shown ? (
    <Icon name="chevronDown" size={FOLD_MARK} color={withAlpha(colors.content, 0.45)} />
  ) : item.harness ? (
    <ProviderLogo harness={item.harness} size={FOLD_MARK} color={withAlpha(colors.content, 0.45)} />
  ) : null;

  return (
    <View>
      <Pressable
        disabled={!expandable}
        onPress={() => {
          void Haptics.selectionAsync();
          setOpen((value) => !value);
        }}
        accessibilityRole={expandable ? "button" : undefined}
        accessibilityState={expandable ? { expanded: shown } : undefined}
        accessibilityLabel={expandable ? (shown ? "Hide the work" : "Show the work") : label}
        style={styles.foldLine}
      >
        {mark}
        {item.live ? (
          <Shimmer style={styles.foldLabel} duration={1}>
            {label}
          </Shimmer>
        ) : (
          <Text style={styles.foldLabel} numberOfLines={1}>
            {label}
          </Text>
        )}
        {/* Out of the chevron, down to the first thing the fold holds. */}
        {shown ? <View style={styles.drop} /> : null}
      </Pressable>
      <Collapsible open={shown}>
        <View style={styles.foldBody}>
          {item.entries.map((entry, index) => (
            <View key={entry.id} style={styles.railRow}>
              <View
                style={[styles.spine, index === item.entries.length - 1 && styles.spineTail]}
              />
              <View style={styles.branch} />
              {entry.type === "prose" ? (
                <View style={styles.foldProse}>
                  <Markdown text={entry.text} muted />
                </View>
              ) : (
                <Phase phase={entry} active={false} />
              )}
            </View>
          ))}
        </View>
      </Collapsible>
    </View>
  );
}

/** A Mono signs its turns with its mascot and name; live, its status runs beside. */
function MonoHeader({ item, agent }: { item: HeaderItem; agent: TranscriptAgent }) {
  const styles = useStyles();
  const signature = (name: string) => (
    <View style={styles.signature}>
      <PixelMascot name={agent.mascot} color={agent.color} size={14} />
      <Text style={styles.signatureName} numberOfLines={1}>
        {name}
      </Text>
    </View>
  );
  if (!item.live) return <View style={styles.monoHeader}>{signature(item.label ?? agent.name)}</View>;
  return (
    <View style={styles.monoHeader}>
      <View style={styles.signatureLive}>{signature(agent.name)}</View>
      <Text style={styles.dot}>·</Text>
      <Shimmer style={styles.monoStatus}>{item.label ?? "Thinking…"}</Shimmer>
    </View>
  );
}

/**
 * The turn's work as phases, each the agent's own line or what its calls add
 * up to, under the icon for what it did most. Live, the phase in progress
 * stays open on its latest steps; tapping any phase lists them.
 */
function WorkPhases({ phases, live }: { phases: WorkPhase[]; live: boolean }) {
  const styles = useStyles();
  return (
    <View style={styles.phases}>
      {phases.map((phase, index) => (
        <Phase key={phase.id} phase={phase} active={live && index === phases.length - 1} />
      ))}
    </View>
  );
}

const KIND_ICONS: Partial<Record<WorkKind, IconName>> = {
  edit: "edit",
  research: "search",
  run: "terminal",
  agent: "robot",
  other: "wrench",
  note: "minus",
};

function Phase({ phase, active }: { phase: WorkPhase; active: boolean }) {
  const colors = useColors();
  const styles = useStyles();
  const [collapsed] = useCollapsedToolCalls();
  const [override, setOverride] = useState<boolean | null>(null);
  const expandable = phase.steps.length > 0;
  const open = expandable && (override ?? (active && !collapsed));
  const icon = KIND_ICONS[phase.kind];
  const steps = active && override === null ? phase.steps.slice(-4) : phase.steps;
  return (
    <View>
      <Pressable
        onPress={() => {
          void Haptics.selectionAsync();
          setOverride(!open);
        }}
        disabled={!expandable}
        accessibilityRole={expandable ? "button" : undefined}
        accessibilityState={expandable ? { expanded: open } : undefined}
        style={styles.phaseRow}
      >
        {icon ? <Icon name={icon} size={PHASE_ICON} color={withAlpha(colors.content, 0.45)} /> : null}
        {active ? (
          <Shimmer style={styles.phaseTitle}>{phase.title}</Shimmer>
        ) : (
          <Text style={styles.phaseTitle} numberOfLines={1}>
            {phase.title}
          </Text>
        )}
      </Pressable>
      <Collapsible open={open}>
        <View style={styles.steps}>
          {steps.map((step, index) => (
            // Each step branches off the phase's rail, as on desktop.
            <View key={step.id} style={styles.stepRow}>
              <View
                style={[styles.stepSpine, index === steps.length - 1 && styles.stepSpineTail]}
              />
              <View style={styles.stepBranch} />
              <Text style={[styles.step, step.failed && styles.stepFailed]} numberOfLines={2}>
                {step.label}
              </Text>
            </View>
          ))}
        </View>
      </Collapsible>
    </View>
  );
}

/** Milliseconds since `startedAt`, ticking each second; null without a start. */
function useElapsed(startedAt: number | undefined) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (startedAt == null) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [startedAt]);
  return startedAt == null ? null : Math.max(0, now - startedAt);
}

const useStyles = themedStyles((colors) => ({
  daySeparator: {
    alignItems: "center",
    paddingTop: 24,
    paddingBottom: 12,
  },
  dayPill: {
    borderRadius: 999,
    backgroundColor: withAlpha(colors.content, 0.07),
    paddingHorizontal: 12,
    paddingVertical: 4,
  },
  dayText: {
    color: withAlpha(colors.content, 0.45),
    fontSize: 12,
    lineHeight: 16,
  },
  dayName: {
    color: withAlpha(colors.content, 0.75),
    fontWeight: "600",
  },
  userRow: {
    alignItems: "flex-end",
    paddingLeft: 56,
    paddingRight: GUTTER,
    paddingTop: 6,
    paddingBottom: 16,
  },
  userRowCompact: {
    paddingTop: 4,
    paddingBottom: 0,
  },
  bubble: {
    maxWidth: "100%",
    paddingHorizontal: 14,
    paddingVertical: 9,
    backgroundColor: colors.bubble,
    borderCurve: "continuous",
  },
  bubbleRound: {
    borderRadius: 21,
  },
  bubbleSquare: {
    borderRadius: 14,
  },
  userText: {
    color: colors.content,
    fontSize: 16,
    lineHeight: 23,
  },
  attachments: {
    color: colors.muted,
    fontSize: 13,
    marginBottom: 4,
  },
  showMore: {
    marginTop: 4,
    color: withAlpha(colors.content, 0.6),
    fontSize: 13,
  },
  userTime: {
    marginTop: 4,
    paddingHorizontal: 12,
    color: withAlpha(colors.content, 0.4),
    fontSize: 12,
  },
  emoji: {
    fontSize: 56,
    lineHeight: 64,
  },
  tail: {
    position: "absolute",
    right: 0,
  },
  reaction: {
    position: "absolute",
    top: -10,
    left: -10,
    height: 22,
    paddingHorizontal: 7,
    borderRadius: 8,
    borderCurve: "continuous",
    justifyContent: "center",
    backgroundColor: colors.surfaceRaised,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: colors.stroke,
  },
  reactionText: {
    fontSize: 11,
    lineHeight: 14,
  },
  foldLine: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    paddingHorizontal: GUTTER,
    paddingVertical: 4,
  },
  foldLabel: {
    flexShrink: 1,
    color: withAlpha(colors.content, 0.5),
    fontSize: WORK_TEXT,
    lineHeight: WORK_LINE,
  },
  drop: {
    position: "absolute",
    left: RAIL_X,
    // The mark's bottom edge: 4 padding plus half the line and half the mark.
    top: 4 + (WORK_LINE + FOLD_MARK) / 2,
    bottom: 0,
    width: 1,
    backgroundColor: colors.rail,
  },
  foldBody: {
    paddingBottom: 4,
  },
  railRow: {
    paddingLeft: CONTENT_X,
    paddingRight: GUTTER,
    paddingBottom: 4,
  },
  spine: {
    position: "absolute",
    left: RAIL_X,
    top: 0,
    bottom: 0,
    width: 1,
    backgroundColor: colors.rail,
  },
  spineTail: {
    bottom: undefined,
    height: BRANCH_TOP + 1,
  },
  branch: {
    position: "absolute",
    left: RAIL_X,
    top: BRANCH_TOP,
    width: 8,
    height: 8,
    borderLeftWidth: 1,
    borderBottomWidth: 1,
    borderColor: colors.rail,
    borderBottomLeftRadius: 8,
  },
  foldProse: {
    paddingVertical: 4,
  },
  pressed: {
    opacity: 0.6,
  },
  monoHeader: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    paddingHorizontal: GUTTER,
    paddingVertical: 4,
  },
  signature: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    alignSelf: "flex-start",
    maxWidth: "100%",
    borderRadius: 999,
    backgroundColor: withAlpha(colors.content, 0.07),
    paddingVertical: 2,
    paddingLeft: 8,
    paddingRight: 12,
  },
  signatureLive: {
    flexShrink: 0,
    maxWidth: "45%",
  },
  signatureName: {
    flexShrink: 1,
    color: withAlpha(colors.content, 0.8),
    fontSize: 14,
    lineHeight: 20,
    fontWeight: "500",
  },
  dot: {
    color: withAlpha(colors.content, 0.25),
    fontSize: 14,
  },
  monoStatus: {
    flex: 1,
    color: withAlpha(colors.content, 0.5),
    fontSize: 14,
    lineHeight: 20,
  },
  work: {
    paddingHorizontal: GUTTER,
  },
  phases: {
    gap: 2,
    paddingVertical: 2,
  },
  phaseRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    paddingVertical: 4,
  },
  phaseTitle: {
    flexShrink: 1,
    color: withAlpha(colors.content, 0.5),
    fontSize: WORK_TEXT,
    lineHeight: WORK_LINE,
  },
  steps: {
    marginBottom: 4,
  },
  // The desktop's .zen-phase-step: the rail under the phase icon's centre, and
  // a quarter-circle branch into each step's first line.
  stepRow: {
    paddingLeft: PHASE_ICON + 6,
    paddingVertical: 4,
  },
  stepSpine: {
    position: "absolute",
    left: PHASE_ICON / 2 - 1,
    top: 0,
    bottom: 0,
    width: 1,
    backgroundColor: colors.rail,
  },
  stepSpineTail: {
    bottom: undefined,
    height: BRANCH_TOP + 1,
  },
  stepBranch: {
    position: "absolute",
    left: PHASE_ICON / 2 - 1,
    top: BRANCH_TOP,
    width: 8,
    height: 8,
    borderLeftWidth: 1,
    borderBottomWidth: 1,
    borderColor: colors.rail,
    borderBottomLeftRadius: 8,
  },
  step: {
    color: withAlpha(colors.content, 0.45),
    fontSize: WORK_TEXT,
    lineHeight: WORK_LINE,
  },
  stepFailed: {
    color: withAlpha(colors.deletions, 0.8),
  },
  reply: {
    paddingHorizontal: GUTTER,
    paddingTop: 12,
    paddingBottom: 4,
  },
  replyUnderLine: {
    paddingTop: 4,
  },
  notice: {
    paddingHorizontal: GUTTER,
    paddingVertical: 8,
    color: withAlpha(colors.content, 0.5),
    fontSize: 14,
    lineHeight: 20,
  },
  errorNotice: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
    marginHorizontal: GUTTER,
    marginVertical: 8,
    padding: 14,
    minHeight: 64,
    borderRadius: 16,
    borderCurve: "continuous",
    backgroundColor: withAlpha(colors.content, 0.04),
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: colors.stroke,
  },
  errorNoticeText: {
    flex: 1,
    gap: 2,
  },
  errorNoticeTitle: {
    color: colors.muted,
    fontSize: 14,
    fontWeight: "500",
    lineHeight: 20,
  },
  errorNoticeHint: {
    color: colors.faint,
    fontSize: 12,
    lineHeight: 17,
  },
  footer: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    paddingLeft: GUTTER - 4,
    paddingRight: GUTTER,
    paddingTop: 4,
    paddingBottom: 12,
  },
  footerActions: {
    flexDirection: "row",
    alignItems: "center",
    gap: 4,
  },
  footerButton: {
    padding: 4,
    borderRadius: 6,
  },
  footerButtonActive: {
    backgroundColor: withAlpha(colors.content, 0.08),
  },
  footerDot: {
    width: 3,
    height: 3,
    borderRadius: 2,
    backgroundColor: withAlpha(colors.content, 0.25),
  },
  footerTime: {
    color: withAlpha(colors.content, 0.35),
    fontSize: 14,
  },
}));

import * as Haptics from "expo-haptics";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { Pressable, ScrollView, Text, View } from "react-native";
import { Gesture, GestureDetector } from "react-native-gesture-handler";
import Animated, {
  Easing,
  interpolate,
  useAnimatedStyle,
  useSharedValue,
  withDelay,
  withSpring,
  withTiming,
} from "react-native-reanimated";
import { scheduleOnRN } from "react-native-worklets";

import type { NewAction } from "@/features/terminal/terminalActions";
import { Icon, type IconName } from "@/ui/Icon";
import {
  GlassPresence,
  LiquidView,
  useGlassEntrance,
  usePopStyle,
} from "@/ui/LiquidView";
import { GLASS_MIN_OPACITY } from "@/ui/SceneFade";
import { radii, themedStyles, useColors, withAlpha } from "@/ui/theme";

import {
  ARROWS,
  ESC,
  INTERRUPT,
  SLASH,
  TAB,
  type Arrow,
  type TerminalKey,
} from "./keys";

/** The bar, the rows that fan out of it, and the space between. */
const BAR = 46;
const GAP = 8;
const SIDE = 8;
const BELOW = 6;
const CHIP = 32;
/** As the running agents' pill comes and goes, and its mini pills rise. */
const PRESENCE_MS = 280;
const STAGGER_MS = 45;
const FAN_SPRING = { damping: 17, stiffness: 210, mass: 0.8 };
/** Held this long, a key offers its alternates. */
const HOLD_MS = 320;
/** One alternate's width in the bubble, and the bubble's inset. */
const CELL = 42;
const BUBBLE_PAD = 4;
/** How far a finger slides on the arrow pad for each arrow. */
const STEP = 14;

/** The rows ••• fans out, nearest the bar first. */
const FAN: TerminalKey[][] = [INTERRUPT.more ?? [], SLASH.more ?? []];
/** Room above the bar for them, which touches pass through while they're away. */
const ROOM = FAN.length * (BAR + GAP);

type Bubble = { left: number; options: TerminalKey[]; choice: number };

/**
 * The keys a phone keyboard lacks, in a glass bar over it, after the running
 * agents' pill: esc, tab, ctrl, ^C, an arrow pad to slide on, / and Paste,
 * with the project's actions as chips above. ^C and / held offer their
 * alternates, slid onto as the keyboard's accents are; ••• fans the same keys
 * out in rows from behind the bar, as the agents' pill does its sessions.
 * Ctrl latches: it applies to the next key typed, then lets go.
 */
export function TerminalKeys({
  ctrl,
  onCtrl,
  onKey,
  onPaste,
  onHideKeyboard,
  actions,
  onAction,
  onPadHeld,
  onHeight,
}: {
  ctrl: boolean;
  onCtrl: () => void;
  onKey: (text: string) => void;
  onPaste: () => void;
  onHideKeyboard: () => void;
  actions: NewAction[];
  onAction: (command: string) => void;
  /** A finger is on the arrow pad, where dragging types arrows. */
  onPadHeld?: (held: boolean) => void;
  /**
   * How far the bar and chips reach up from the bottom, without the room the
   * rows fan into.
   */
  onHeight: (height: number) => void;
}) {
  const styles = useStyles();
  const [open, setOpen] = useState(false);
  const [bubble, setBubble] = useState<Bubble | null>(null);
  const shown = useRef<Bubble | null>(null);
  const showBubble = (next: Bubble | null) => {
    shown.current = next;
    setBubble(next);
  };
  // Where each key with alternates sits in the bar, for its bubble.
  const keyX = useRef(new Map<string, number>());
  const barWidth = useRef(0);

  const holdKey = (key: TerminalKey, startX: number) => {
    const options = [key, ...(key.more ?? [])];
    const width = options.length * CELL + BUBBLE_PAD * 2;
    const at = SIDE + (keyX.current.get(key.name) ?? 0);
    const left = Math.max(
      SIDE,
      Math.min(SIDE + barWidth.current - width, at - BUBBLE_PAD),
    );
    void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light);
    showBubble({ left, options, choice: 0 });
    return { left, startX: at + startX };
  };
  const slideBubble = (fingerX: number, left: number) => {
    const current = shown.current;
    if (!current) return;
    const choice = Math.max(
      0,
      Math.min(
        current.options.length - 1,
        Math.floor((fingerX - left - BUBBLE_PAD) / CELL),
      ),
    );
    if (choice === current.choice) return;
    void Haptics.selectionAsync();
    showBubble({ ...current, choice });
  };
  const releaseBubble = (type: boolean) => {
    const current = shown.current;
    showBubble(null);
    if (type && current) onKey(current.options[current.choice].text);
  };
  const held = { holdKey, slideBubble, releaseBubble };

  return (
    <View style={styles.root} pointerEvents="box-none">
      <View
        onLayout={(event) => onHeight(event.nativeEvent.layout.height + BELOW)}
      >
        {actions.length > 0 ? (
          <Chips open={open}>
            <ScrollView
              horizontal
              keyboardShouldPersistTaps="always"
              showsHorizontalScrollIndicator={false}
              contentContainerStyle={styles.chips}
            >
              {actions.map((action, index) => (
                <LiquidView
                  key={action.id ?? `${index}`}
                  borderRadius={radii.pill}
                  innerStyle={styles.chip}
                  pressedScale={1.04}
                  onPress={() => onAction(action.command)}
                  accessibilityLabel={`Run ${action.name}`}
                >
                  <Icon name="play" size={12} />
                  <Text style={styles.chipLabel} numberOfLines={1}>
                    {action.name}
                  </Text>
                </LiquidView>
              ))}
            </ScrollView>
          </Chips>
        ) : null}
        <View style={styles.row}>
          <View
            style={styles.grow}
            onLayout={(event) =>
              (barWidth.current = event.nativeEvent.layout.width)
            }
          >
            <LiquidView borderRadius={radii.pill} innerStyle={styles.bar}>
              <TypeKey terminalKey={ESC} onKey={onKey} />
              <TypeKey terminalKey={TAB} onKey={onKey} />
              <TextKey label="ctrl" name="Control" on={ctrl} onPress={onCtrl} />
              <HoldKey
                terminalKey={INTERRUPT}
                onKey={onKey}
                held={held}
                onX={(x) => keyX.current.set(INTERRUPT.name, x)}
              />
              <ArrowPad
                onArrow={(arrow) => onKey(ARROWS[arrow])}
                onHeld={onPadHeld}
              />
              <HoldKey
                terminalKey={SLASH}
                onKey={onKey}
                held={held}
                onX={(x) => keyX.current.set(SLASH.name, x)}
              />
              <IconKey icon="paste" label="Paste" onPress={onPaste} />
            </LiquidView>
          </View>
          <MoreButton open={open} onPress={() => setOpen((was) => !was)} />
        </View>
      </View>
      {/* After the bar and chips, so the rows rise over them. */}
      <FanRows open={open} onKey={onKey} onHideKeyboard={onHideKeyboard} />
      {bubble ? <AlternatesBubble bubble={bubble} /> : null}
    </View>
  );
}

/**
 * The action chips, which sink into the bar and clear while ••• has its rows
 * out, so the rows rise where they were rather than through them.
 */
function Chips({ open, children }: { open: boolean; children: ReactNode }) {
  const away = useSharedValue(0);
  useEffect(() => {
    away.set(
      open
        ? withTiming(1, { duration: 160, easing: Easing.in(Easing.cubic) })
        : withDelay(STAGGER_MS * FAN.length, withSpring(0, FAN_SPRING)),
    );
  }, [open, away]);
  const style = useAnimatedStyle(() => ({
    opacity: Math.max(GLASS_MIN_OPACITY, 1 - away.get()),
    transform: [
      { translateY: away.get() * (CHIP / 2) },
      { scale: 1 - away.get() * 0.1 },
    ],
  }));
  return (
    <Animated.View style={style} pointerEvents={open ? "none" : "auto"}>
      {children}
    </Animated.View>
  );
}

/**
 * The rows ••• opens: each rises out from behind the bar in turn, the nearest
 * first, and sinks back into it, the farthest first, as the running agents'
 * mini pills do. Their glass forms and clears with them.
 */
function FanRows({
  open,
  onKey,
  onHideKeyboard,
}: {
  open: boolean;
  onKey: (text: string) => void;
  onHideKeyboard: () => void;
}) {
  const { mounted, visible, presence } = useGlassEntrance(open, PRESENCE_MS);
  if (!mounted) return null;
  return (
    <GlassPresence value={presence}>
      {FAN.map((keys, place) => (
        <FanRow key={place} place={place} count={FAN.length} visible={visible}>
          {keys.map((key) => (
            <TypeKey key={key.name} terminalKey={key} onKey={onKey} />
          ))}
          {place === 0 ? (
            <IconKey
              icon="keyboardOff"
              label="Hide keyboard"
              onPress={onHideKeyboard}
            />
          ) : null}
        </FanRow>
      ))}
    </GlassPresence>
  );
}

function FanRow({
  place,
  count,
  visible,
  children,
}: {
  /** How many rows up from the bar, the nearest 0. */
  place: number;
  count: number;
  visible: boolean;
  children: ReactNode;
}) {
  const styles = useStyles();
  const rise = useSharedValue(0);
  useEffect(() => {
    const order = visible ? place : count - 1 - place;
    rise.set(
      withDelay(
        order * STAGGER_MS,
        visible
          ? withSpring(1, FAN_SPRING)
          : withTiming(0, {
              duration: PRESENCE_MS - 60,
              easing: Easing.in(Easing.cubic),
            }),
      ),
    );
  }, [visible, place, count, rise]);
  const style = useAnimatedStyle(() => {
    const t = rise.get();
    const tucked = (place + 1) * (BAR + GAP);
    return {
      // Never quite clear, or its glass comes up without its effect.
      opacity: Math.max(GLASS_MIN_OPACITY, Math.min(1, t / 0.5)),
      transform: [
        { translateY: interpolate(t, [0, 1], [tucked, 0]) },
        { scale: interpolate(t, [0, 1], [0.8, 1]) },
      ],
    };
  });
  return (
    <Animated.View
      style={[
        styles.fanRow,
        { bottom: BELOW + (place + 1) * (BAR + GAP) },
        style,
      ]}
      pointerEvents={visible ? "box-none" : "none"}
    >
      <LiquidView borderRadius={radii.pill} innerStyle={styles.bar}>
        {children}
      </LiquidView>
    </Animated.View>
  );
}

/** ••• that turns into a close, its glass growing a little as it's pressed. */
function MoreButton({ open, onPress }: { open: boolean; onPress: () => void }) {
  const styles = useStyles();
  const turn = useSharedValue(0);
  useEffect(() => {
    turn.set(withSpring(open ? 1 : 0, FAN_SPRING));
  }, [open, turn]);
  const dots = useAnimatedStyle(() => ({
    opacity: 1 - turn.get(),
    transform: [
      { rotate: `${turn.get() * 90}deg` },
      { scale: 1 - turn.get() * 0.4 },
    ],
  }));
  const close = useAnimatedStyle(() => ({
    opacity: turn.get(),
    transform: [
      { rotate: `${(turn.get() - 1) * 90}deg` },
      { scale: 0.6 + turn.get() * 0.4 },
    ],
  }));
  return (
    <LiquidView
      borderRadius={BAR / 2}
      innerStyle={styles.more}
      onPress={onPress}
      pressedScale={1.08}
      accessibilityLabel={open ? "Fewer keys" : "More keys"}
    >
      <Animated.Text style={[styles.dots, dots]}>•••</Animated.Text>
      <Animated.View style={[styles.closeIcon, close]}>
        <Icon name="close" size={18} />
      </Animated.View>
    </LiquidView>
  );
}

/** A held key's alternates, popping up over it as the keyboard's accents do. */
function AlternatesBubble({ bubble }: { bubble: Bubble }) {
  const styles = useStyles();
  const pop = usePopStyle(true, PRESENCE_MS);
  return (
    <Animated.View
      style={[styles.bubble, { left: bubble.left }, pop]}
      pointerEvents="none"
    >
      <LiquidView borderRadius={radii.pill} innerStyle={styles.bubbleGlass}>
        {bubble.options.map((option, index) => (
          <View
            key={option.name}
            style={[styles.option, index === bubble.choice && styles.chosen]}
          >
            <Text
              style={[
                styles.label,
                index === bubble.choice && styles.chosenLabel,
              ]}
            >
              {option.label}
            </Text>
          </View>
        ))}
      </LiquidView>
    </Animated.View>
  );
}

/** A key that types the moment it's touched. */
function TypeKey({
  terminalKey: key,
  onKey,
}: {
  terminalKey: TerminalKey;
  onKey: (text: string) => void;
}) {
  const styles = useStyles();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={key.name}
      onPressIn={() => {
        void Haptics.selectionAsync();
        onKey(key.text);
      }}
      style={({ pressed }) => [styles.key, pressed && styles.pressed]}
    >
      <Text style={[styles.label, key.label.length === 1 && styles.glyph]}>
        {key.label}
      </Text>
    </Pressable>
  );
}

type Held = {
  holdKey: (
    key: TerminalKey,
    startX: number,
  ) => { left: number; startX: number };
  slideBubble: (fingerX: number, left: number) => void;
  releaseBubble: (type: boolean) => void;
};

/**
 * A key with alternates: a tap types it; held, its alternates pop up and the
 * one slid onto is typed on letting go.
 */
function HoldKey({
  terminalKey: key,
  onKey,
  held,
  onX,
}: {
  terminalKey: TerminalKey;
  onKey: (text: string) => void;
  held: Held;
  onX: (x: number) => void;
}) {
  const styles = useStyles();
  const [down, setDown] = useState(false);
  const latest = useRef({ key, onKey, held });
  useEffect(() => {
    latest.current = { key, onKey, held };
  });
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const open = useRef<{ left: number; startX: number } | null>(null);
  const begin = useCallback((x: number) => {
    setDown(true);
    void Haptics.selectionAsync();
    timer.current = setTimeout(() => {
      timer.current = null;
      open.current = latest.current.held.holdKey(latest.current.key, x);
    }, HOLD_MS);
  }, []);
  const slide = useCallback((dx: number) => {
    const at = open.current;
    if (at) latest.current.held.slideBubble(at.startX + dx, at.left);
  }, []);
  const end = useCallback(() => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
    setDown(false);
    const { key: current, onKey: typeKey, held: hold } = latest.current;
    if (open.current) hold.releaseBubble(true);
    else typeKey(current.text);
    open.current = null;
  }, []);
  // Built once: a new gesture each render would be attached anew. Its
  // callbacks read refs only as the gesture fires, never while rendering.
  /* eslint-disable react-hooks/refs */
  const press = useMemo(
    () =>
      Gesture.Pan()
        .minDistance(0)
        .shouldCancelWhenOutside(false)
        .onBegin((event) => scheduleOnRN(begin, event.x))
        .onUpdate((event) => scheduleOnRN(slide, event.translationX))
        .onFinalize(() => scheduleOnRN(end)),
    [begin, slide, end],
  );
  /* eslint-enable react-hooks/refs */
  return (
    <GestureDetector gesture={press}>
      <View
        accessibilityRole="button"
        accessibilityLabel={`${key.name}. Hold for more`}
        onLayout={(event) => onX(event.nativeEvent.layout.x)}
        style={[styles.key, down && styles.pressed]}
      >
        <Text style={[styles.label, key.label.length === 1 && styles.glyph]}>
          {key.label}
        </Text>
        <View style={styles.moreDot} />
      </View>
    </GestureDetector>
  );
}

/**
 * Four arrow keys in one: slide a finger on it and each step it moves types
 * the arrow that way, so moving through history or a line is one drag.
 */
function ArrowPad({
  onArrow,
  onHeld,
}: {
  onArrow: (arrow: Arrow) => void;
  onHeld?: (held: boolean) => void;
}) {
  const styles = useStyles();
  const colors = useColors();
  const [hit, setHit] = useState<Arrow | null>(null);
  const latest = useRef({ onArrow, onHeld });
  useEffect(() => {
    latest.current = { onArrow, onHeld };
  });
  const clear = useRef<ReturnType<typeof setTimeout> | null>(null);
  const fire = useCallback((arrow: Arrow) => {
    void Haptics.selectionAsync();
    latest.current.onArrow(arrow);
    setHit(arrow);
    if (clear.current) clearTimeout(clear.current);
    clear.current = setTimeout(() => setHit(null), 120);
  }, []);
  const hold = useCallback(
    (held: boolean) => latest.current.onHeld?.(held),
    [],
  );
  // Where the last arrow was typed, so each further step types another.
  const fromX = useSharedValue(0);
  const fromY = useSharedValue(0);
  // Its callbacks read refs only as the gesture fires, never while rendering.
  /* eslint-disable react-hooks/refs */
  const slide = useMemo(
    () =>
      Gesture.Pan()
        .minDistance(0)
        .shouldCancelWhenOutside(false)
        .onBegin(() => {
          fromX.set(0);
          fromY.set(0);
          scheduleOnRN(hold, true);
        })
        .onUpdate((event) => {
          const dx = event.translationX - fromX.get();
          const dy = event.translationY - fromY.get();
          if (Math.abs(dx) < STEP && Math.abs(dy) < STEP) return;
          if (Math.abs(dx) > Math.abs(dy))
            scheduleOnRN(fire, dx > 0 ? "right" : "left");
          else scheduleOnRN(fire, dy > 0 ? "down" : "up");
          fromX.set(event.translationX);
          fromY.set(event.translationY);
        })
        .onFinalize(() => scheduleOnRN(hold, false)),
    [fire, hold, fromX, fromY],
  );
  /* eslint-enable react-hooks/refs */
  const arrow = (which: Arrow, glyph: string, style: object) => (
    <Text
      style={[
        styles.arrow,
        style,
        {
          color:
            hit === which ? colors.content : withAlpha(colors.content, 0.5),
        },
      ]}
    >
      {glyph}
    </Text>
  );
  return (
    <GestureDetector gesture={slide}>
      <View
        accessibilityRole="adjustable"
        accessibilityLabel="Arrow keys. Slide to move"
        style={styles.pad}
        // Not the sheet's: a drag here moves the cursor.
        onTouchStart={(event) => event.stopPropagation()}
      >
        {arrow("up", "▲", styles.up)}
        {arrow("down", "▼", styles.down)}
        {arrow("left", "◀", styles.left)}
        {arrow("right", "▶", styles.right)}
        <View style={styles.padDot} />
      </View>
    </GestureDetector>
  );
}

function TextKey({
  label,
  name,
  on,
  onPress,
}: {
  label: string;
  name: string;
  on?: boolean;
  onPress: () => void;
}) {
  const styles = useStyles();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={name}
      accessibilityState={on === undefined ? undefined : { selected: on }}
      onPressIn={() => void Haptics.selectionAsync()}
      onPress={onPress}
      style={({ pressed }) => [
        styles.key,
        (pressed || on) && styles.pressed,
        on && styles.on,
      ]}
    >
      <Text style={[styles.label, on && styles.chosenLabel]}>{label}</Text>
    </Pressable>
  );
}

function IconKey({
  icon,
  label,
  onPress,
}: {
  icon: IconName;
  label: string;
  onPress: () => void;
}) {
  const styles = useStyles();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      onPressIn={() => void Haptics.selectionAsync()}
      onPress={onPress}
      style={({ pressed }) => [styles.key, pressed && styles.pressed]}
    >
      <Icon name={icon} size={18} />
    </Pressable>
  );
}

const useStyles = themedStyles((colors) => ({
  root: { paddingTop: ROOM, paddingBottom: BELOW },
  chips: { gap: 6, paddingHorizontal: SIDE, paddingBottom: GAP },
  chip: { height: CHIP, paddingHorizontal: 12, gap: 6, alignItems: "center" },
  chipLabel: { color: colors.content, fontSize: 13, fontWeight: "500" },
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: GAP,
    paddingHorizontal: SIDE,
  },
  grow: { flex: 1 },
  bar: { height: BAR, paddingHorizontal: 4, alignItems: "center" },
  fanRow: { position: "absolute", left: SIDE, right: SIDE + BAR + GAP },
  more: {
    width: BAR,
    height: BAR,
    alignItems: "center",
    justifyContent: "center",
  },
  dots: {
    position: "absolute",
    color: colors.content,
    fontSize: 14,
    fontWeight: "700",
    letterSpacing: 1,
  },
  closeIcon: { position: "absolute" },
  key: {
    flex: 1,
    height: 38,
    marginHorizontal: 1,
    borderRadius: 19,
    alignItems: "center",
    justifyContent: "center",
  },
  pressed: { backgroundColor: withAlpha(colors.content, 0.12) },
  on: { backgroundColor: colors.accent },
  label: { color: colors.content, fontSize: 14, fontWeight: "500" },
  glyph: { fontSize: 18 },
  chosenLabel: { color: "#ffffff" },
  moreDot: {
    position: "absolute",
    top: 7,
    right: 6,
    width: 4,
    height: 4,
    borderRadius: 2,
    backgroundColor: withAlpha(colors.content, 0.45),
  },
  pad: {
    flex: 2.1,
    height: 38,
    marginHorizontal: 2,
    borderRadius: 19,
    backgroundColor: withAlpha(colors.content, 0.07),
  },
  arrow: { position: "absolute", fontSize: 9 },
  up: { top: 2, alignSelf: "center" },
  down: { bottom: 2, alignSelf: "center" },
  left: { left: 10, top: 12 },
  right: { right: 10, top: 12 },
  padDot: {
    position: "absolute",
    left: "50%",
    top: "50%",
    width: 10,
    height: 10,
    marginLeft: -5,
    marginTop: -5,
    borderRadius: 5,
    backgroundColor: withAlpha(colors.content, 0.5),
  },
  bubble: { position: "absolute", bottom: BELOW + BAR + 6 },
  bubbleGlass: {
    height: 48,
    padding: BUBBLE_PAD,
    gap: 0,
    alignItems: "center",
  },
  option: {
    width: CELL,
    height: 40,
    borderRadius: 20,
    alignItems: "center",
    justifyContent: "center",
  },
  chosen: { backgroundColor: colors.accent },
}));

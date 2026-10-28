import { router, useGlobalSearchParams, useIsFocused, usePathname } from "expo-router";
import { BlurTargetView } from "expo-blur";
import {
  createContext,
  useCallback,
  useEffect,
  use,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import {
  Platform,
  Text,
  View,
  type LayoutChangeEvent,
  type StyleProp,
  type ViewStyle,
} from "react-native";
import {
  KeyboardGestureArea,
  useKeyboardHandler,
} from "react-native-keyboard-controller";
import Animated, {
  Easing,
  interpolate,
  useAnimatedStyle,
  useSharedValue,
  withTiming,
  type AnimatedStyle,
  type SharedValue,
} from "react-native-reanimated";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { CircleButton } from "@/ui/CircleButton";
import { GradualBlur } from "@/ui/GradualBlur";
import { Icon, type IconName } from "@/ui/Icon";
import {
  GLASS_PUSH_DURATION,
  HEADER_PUSH_TOTAL,
  PUSH_DONE_AFTER,
  hasNativeHeader,
  isModalRoute,
  useHeaderPush,
  useSceneKey,
} from "@/ui/SceneFade";
import { GlassBlurTarget, GlassPresence, LiquidView } from "@/ui/LiquidView";
import { themedStyles, useColors } from "@/ui/theme";

const HEADER_HEIGHT = 44;
/** The scroll-to-end button above the composer. */
const JUMP = 36;
/** Gap between the composer and the keyboard while it is open. */
const KEYBOARD_GAP = 8;
/** How far past the header and composer the blur keeps fading. Content rests
 *  clear of it (see `top` and `bottom`), so it only dims once scrolled underneath. */
const FADE = 16;
/** The header's fade reaches further, so content softens well before it goes under. */
const TOP_FADE = 36;
/** How far the blur reaches up a page without the composer. */
const PAGE_FADE = 40;
/** How far below the header the Changes and Files pages start. Their first row
 *  centres its text in a list-row height, which keeps the text clear of the fade. */
const PAGE_TOP_GAP = 8;
/** The line under the header, when a screen has one; pages start below it. */
const BELOW_HEIGHT = 22;
/** Space between the header and that line. */
const BELOW_GAP = 2;
/** Matches `nativeID` on the composer input, for interactive keyboard dismissal. */
export const COMPOSER_INPUT_ID = "composer-input";
/** How far the layout's `bottom` reaches past the composer's top, for a view resting on it. */
export const COMPOSER_FADE = FADE;

type SessionLayout = {
  /** Space taken by the floating header and its blur; content pads its top by this. */
  top: number;
  /** The tighter top for list pages (Changes, Files), whose first row has room above its text. */
  pageTop: number;
  /** Space taken by the composer at its resting height, the safe area and the blur. */
  bottom: number;
  /** For a page the composer slides away from (see `dockStyle`): the bottom
   *  safe area and a blur over it, which PageBottomBlur draws. */
  bareBottom: number;
  /** Where the composer rests above the screen's bottom edge, for a page's own bottom bar. */
  dockBottom: number;
  /** How much less than the keyboard height the composer travels; KeyboardChatScrollView `offset`. */
  keyboardOffset: number;
  /** Extra height while the composer grows past one line; KeyboardChatScrollView `extraContentPadding`. */
  extraContentPadding: SharedValue<number> | undefined;
  /** Latest measured growth, for native transcripts to adjust their own inset. */
  composerGrowth: number;
  /** False until the screen's chrome is in and the composer measured, so
   *  `bottom` won't change under content centred by it. */
  settled: boolean;
};

const SessionLayoutContext = createContext<SessionLayout>({
  top: 0,
  pageTop: 0,
  bottom: 0,
  bareBottom: 0,
  dockBottom: 0,
  keyboardOffset: 0,
  extraContentPadding: undefined,
  composerGrowth: 0,
  settled: true,
});

const ContentBlurTarget = createContext<RefObject<View | null> | undefined>(undefined);
const ContentBlurReady = createContext<(() => void) | undefined>(undefined);

/** Record page content only; native header declarations must stay outside. */
export function SessionContent({ children }: { children: ReactNode }) {
  const targetRef = use(ContentBlurTarget);
  const onLayout = use(ContentBlurReady);
  if (!targetRef) return <>{children}</>;
  return (
    <BlurTargetView ref={targetRef} onLayout={onLayout} style={{ flex: 1 }}>
      {/* A surface inside the recorded content must never sample its ancestor. */}
      <GlassBlurTarget value={undefined}>{children}</GlassBlurTarget>
    </BlurTargetView>
  );
}

/** The composer's one-line height, as last measured, so a new frame starts
 *  with it rather than with nothing and a jump once the composer lays out. */
let lastRestingHeight = 0;

/** The bottom blur of a page the composer slides away from, as tall as its
 *  `bareBottom` padding, so content rests just clear of it. */
export function PageBottomBlur() {
  const colors = useColors();
  const { bareBottom } = use(SessionLayoutContext);
  return (
    <GradualBlur edge="bottom" height={bareBottom} color={colors.background} />
  );
}

/** Insets for content drawn full-bleed under the header and composer. */
export function useSessionLayout() {
  return use(SessionLayoutContext);
}

/**
 * Insets for a page shown in a sheet rather than in the frame, as Changes is
 * over a session: its own space at the top, and the safe area at the bottom
 * with no composer above it.
 */
export function SheetLayout({
  top,
  children,
}: {
  top: number;
  children: ReactNode;
}) {
  const insets = useSafeAreaInsets();
  const dockBottom = Math.max(insets.bottom, 12);
  const layout = useMemo<SessionLayout>(
    () => ({
      top,
      pageTop: top,
      bottom: dockBottom,
      bareBottom: dockBottom + PAGE_FADE,
      dockBottom,
      keyboardOffset: 0,
      extraContentPadding: undefined,
      composerGrowth: 0,
      settled: true,
    }),
    [top, dockBottom],
  );
  return <SessionLayoutContext value={layout}>{children}</SessionLayoutContext>;
}

/**
 * The composer's scroll-to-end button. The transcript owns the scrolling, so
 * it fades the button in when the reader leaves the end and hands over how to
 * get back there.
 */
type JumpToEnd = {
  /** 0 hidden, 1 shown. */
  visible: SharedValue<number>;
  setTarget: (scrollToEnd: (() => void) | null) => void;
};

const JumpToEndContext = createContext<JumpToEnd | null>(null);

export function useJumpToEnd() {
  return use(JumpToEndContext);
}

/** What a screen puts in the persistent frame around it. */

export type Chrome = {
  /** Plain header title; omit for a bare header. */
  title?: string;
  subtitle?: string;
  /** Glass pill in the header, e.g. a mono's mascot and name, or a session's
   * title over its project and branch. Wins over `title`. */
  pill?: { icon?: ReactNode; label: string; subtitle?: string };
  /** Any element for the header's centre, e.g. a view switch. Wins over `pill` and `title`. */
  center?: ReactNode;
  /** Screens with the same key share the header's centre and the line under
   *  it, which then stay put moving between them, as from an empty session
   *  to a session. */
  headerKey?: string;
  /** The header's centre and the line under it appear in place on arrival,
   *  rather than pushing in with the rest of the header. */
  instantCenter?: boolean;
  /** Round button on the far right of the header. */
  action?: { icon: IconName; label: string; onPress?: () => void };
  /** Any element for the header's far right, e.g. a menu. Wins over `action`. */
  trailing?: ReactNode;
  /** A short line centred under the header, e.g. the branch and its changes;
   *  it stays put while pages slide beneath it. */
  below?: ReactNode;
  /** Omit for a screen without one; content then runs to the bottom safe area. */
  composer?: ReactNode;
  /** Moves the composer with its blur and scroll button, e.g. off with a page
   *  that has it; pages without it then pad by `bareBottom`. */
  dockStyle?: StyleProp<AnimatedStyle<ViewStyle>>;
};

/** Publishes a screen's chrome, filed under its route. */
const ChromeContext = createContext<(path: string, chrome: Chrome) => void>(
  () => {},
);

/**
 * A main-pane screen. Hands its header text and composer to the persistent
 * SessionFrame and renders only its content, so moving between screens (say a
 * mono and a project) swaps the content without rebuilding the header, composer
 * and blur around it. Set in a layout effect, so the frame updates before paint.
 */
export function SessionScreen({
  children,
  title,
  subtitle,
  pill,
  center,
  headerKey,
  instantCenter,
  action,
  trailing,
  below,
  composer,
  dockStyle,
}: Chrome & {
  children: ReactNode;
}) {
  const publish = use(ChromeContext);
  // The app's route, not this screen's: a screen on its way out re-renders
  // with the next one's, so only the focused screen may publish, or the one
  // leaving would file its header under the next route over the right one.
  const path = useSceneKey();
  const focused = useIsFocused();
  useLayoutEffect(
    () => {
      if (!focused) return;
      publish(path, {
        title,
        subtitle,
        pill,
        center,
        headerKey,
        instantCenter,
        action,
        trailing,
        below,
        composer,
        dockStyle,
      });
    },
    [
      publish,
      focused,
      path,
      title,
      subtitle,
      pill,
      center,
      headerKey,
      instantCenter,
      action,
      trailing,
      below,
      composer,
      dockStyle,
    ],
  );
  return children;
}

// Persistent frame around every main-pane screen, mounted once in the root
// layout. Laid out as in the keyboard-controller chat guide: content runs
// full-bleed under a floating header, and the composer sits in a
// view that rides on top of the keyboard (see useKeyboardLift).
export function SessionFrame({
  children,
  nativeHeaderHeight,
}: {
  children: ReactNode;
  /** For a screen under the stack's native header: that header's height.
   *  It takes the place of this frame's own. */
  nativeHeaderHeight?: number;
}) {
  const colors = useColors();
  const styles = useStyles();
  const native = nativeHeaderHeight !== undefined;
  // A screen under a native header draws its own frame inside this one;
  // this one's header and composer stand aside meanwhile.
  // The screen under a modal still has it, so the frame's own header doesn't
  // show through the modal's while it is up.
  const pathname = usePathname();
  const [scene, setScene] = useState(pathname);
  if (scene !== pathname && !isModalRoute(pathname)) setScene(pathname);
  const covered = !native && hasNativeHeader(scene);
  // Each route's chrome, and the last route's while the header pushes it out.
  const [header, setHeader] = useState<{
    path: string;
    chrome: Chrome;
    previous: { path: string; chrome: Chrome } | null;
  } | null>(null);
  const chrome = header?.chrome ?? null;
  const setChrome = useCallback((path: string, next: Chrome) => {
    setHeader((prev) =>
      !prev || prev.path === path
        ? { path, chrome: next, previous: prev?.previous ?? null }
        : { path, chrome: next, previous: { path: prev.path, chrome: prev.chrome } },
    );
  }, []);

  // The header's own push, run as soon as a new screen publishes its pieces.
  // They are drawn hidden first, then the push starts two frames on.
  const push = useSharedValue(HEADER_PUSH_TOTAL);
  const [pushed, setPushed] = useState<string | null>(null);
  const headerPath = header?.path;
  useLayoutEffect(() => {
    if (headerPath) push.set(0);
  }, [headerPath, push]);
  useEffect(() => {
    if (!headerPath) return;
    let frame = requestAnimationFrame(() => {
      frame = requestAnimationFrame(() => {
        setPushed(headerPath);
        push.set(
          withTiming(HEADER_PUSH_TOTAL, {
            duration: HEADER_PUSH_TOTAL,
            easing: Easing.linear,
          }),
        );
      });
    });
    return () => cancelAnimationFrame(frame);
  }, [headerPath, push]);
  // Whether the current screen's push has started. Until then its pieces are
  // held hidden and the last screen's stay as they were.
  const started = !!header && pushed === header.path;
  const pushing = !!header?.previous && started;
  useEffect(() => {
    if (!pushing) return;
    const timer = setTimeout(
      () => setHeader((prev) => prev && { ...prev, previous: null }),
      PUSH_DONE_AFTER,
    );
    return () => clearTimeout(timer);
  }, [pushing]);
  // Newest first, so a route's pieces keep their place as the next one's go
  // in ahead of them and never remount mid push.
  const layers = header
    ? [
        { path: header.path, chrome: header.chrome, incoming: true },
        ...(header.previous ? [{ ...header.previous, incoming: false }] : []),
      ]
    : [];
  // The centre and the line under it stay put between screens sharing them.
  const shared =
    layers.length === 2 &&
    !!layers[0].chrome.headerKey &&
    layers[0].chrome.headerKey === layers[1].chrome.headerKey;
  // A screen whose centre appears in place drops the last one's at once.
  const centerLayers =
    shared || layers[0]?.chrome.instantCenter
    ? [{ ...layers[0], key: centerKey(layers[0]), still: true }]
    : layers.map((layer) => ({
        ...layer,
        key: centerKey(layer),
        still: false,
      }));
  // The right of the header only rolls when it changes, say from a mono's
  // button to a project's. Between screens sharing the centre it swaps in
  // place, as from a session's new-session button to the empty session's
  // background one, and moving between sessions keeps the one button.
  const trailingLayers =
    shared ||
    (layers.length === 2 &&
      trailingKey(layers[0].path, layers[0].chrome) ===
        trailingKey(layers[1].path, layers[1].chrome))
      ? [{ ...layers[0], key: sharedTrailingKey(layers[0].path, layers[0].chrome), still: true }]
      : layers.map((layer) => ({
          ...layer,
          key: sharedTrailingKey(layer.path, layer.chrome),
          still: false,
        }));
  // The menu button opens the project picker, the current project checked.
  const { projectId } = useGlobalSearchParams<{ projectId?: string }>();
  const insets = useSafeAreaInsets();
  const extraContentPadding = useSharedValue(0);
  const restingHeight = useRef<number | null>(null);
  const [composerHeight, setComposerHeight] = useState(lastRestingHeight);
  const [composerGrowth, setComposerGrowth] = useState(0);
  const jumpVisible = useSharedValue(0);
  const jumpTarget = useRef<(() => void) | null>(null);
  const [hasJumpTarget, setHasJumpTarget] = useState(false);
  const jump = useMemo<JumpToEnd>(
    () => ({
      visible: jumpVisible,
      setTarget: (scrollToEnd) => {
        jumpTarget.current = scrollToEnd;
        setHasJumpTarget(scrollToEnd !== null);
      },
    }),
    [jumpVisible],
  );
  const jumpStyle = useAnimatedStyle(() => ({
    // Fully remove the native glass surface while hidden; opacity alone can
    // leave its backdrop behind on iOS.
    display: jumpVisible.get() === 0 ? "none" : "flex",
    opacity: jumpVisible.get(),
    // The composer's growth past its resting height, as it expands on focus
    // or wraps, so the button rides its top edge rather than sitting over it.
    transform: [
      { translateY: -extraContentPadding.get() },
      { scale: interpolate(jumpVisible.get(), [0, 1], [0.8, 1]) },
    ],
  }));

  const headerTop = insets.top + 6;
  // The outer frame contains the native navigator and its header surfaces.
  // Recording that whole tree can include a blur sampling its own RenderNode.
  // Only the per-screen frame may record content for its sibling surfaces.
  const androidBlur = Platform.OS === "android" && native;
  const blurTarget = useRef<View>(null);
  const [blurTargetReady, setBlurTargetReady] = useState(false);
  const onBlurTargetLayout = useCallback(() => {
    if (blurTarget.current) setBlurTargetReady(true);
  }, []);
  const headerBottom = headerTop + HEADER_HEIGHT + 8;
  // Android's workspace menu is a React Native control with its own padding
  // and font metrics. Let it lay out fully rather than squeezing it into the
  // compact iOS line height, and reserve that same room for page content.
  const measureBelow = native && Platform.OS === "android";
  const [belowHeight, setBelowHeight] = useState(BELOW_HEIGHT);
  const hasBelow = !!chrome?.below;
  // Where the frame above the content ends: the header, and the line under it if any.
  const belowTop = (native ? nativeHeaderHeight : headerBottom) + BELOW_GAP;
  const frameBottom = hasBelow
    ? belowTop + (measureBelow ? belowHeight : BELOW_HEIGHT)
    : native
      ? nativeHeaderHeight
      : headerBottom;
  const composerBottom = Math.max(insets.bottom, 12);
  const keyboardOffset = composerBottom - KEYBOARD_GAP;
  const keyboardLift = useKeyboardLift(keyboardOffset);
  const contentBottom = chrome?.composer
    ? composerHeight + composerBottom + FADE
    : composerBottom;
  const settled =
    !!chrome &&
    (!chrome.composer || composerHeight > 0) &&
    (!native || nativeHeaderHeight > 0);
  // Publishing new chrome must not invalidate every content consumer. Settings
  // reads these insets and publishes chrome itself, so an unstable value loops.
  const layout = useMemo<SessionLayout>(
    () => ({
      top: frameBottom + TOP_FADE,
      pageTop: frameBottom + PAGE_TOP_GAP,
      bottom: contentBottom,
      bareBottom: composerBottom + PAGE_FADE,
      dockBottom: composerBottom,
      keyboardOffset,
      extraContentPadding,
      composerGrowth,
      settled,
    }),
    [
      frameBottom,
      contentBottom,
      composerBottom,
      keyboardOffset,
      extraContentPadding,
      composerGrowth,
      settled,
    ],
  );

  // The first layout is the one-line resting height. Growth past it goes to
  // extraContentPadding, so the list extends its range without re-laying out.
  const onComposerLayout = (event: LayoutChangeEvent) => {
    const height = Math.round(event.nativeEvent.layout.height);
    if (height <= 0) return;
    if (restingHeight.current === null) {
      restingHeight.current = height;
      lastRestingHeight = height;
      setComposerHeight(height);
      return;
    }
    const growth = Math.max(0, height - restingHeight.current);
    setComposerGrowth(growth);
    extraContentPadding.set(
      withTiming(growth, {
        duration: 180,
        easing: Easing.out(Easing.cubic),
      }),
    );
  };

  return (
    <ChromeContext value={setChrome}>
      <JumpToEndContext value={jump}>
        <SessionLayoutContext value={layout}>
          <KeyboardGestureArea
            interpolator="ios"
            textInputNativeID={COMPOSER_INPUT_ID}
            style={styles.scene}
          >
            {/* First, unwrapped: the screens' scroll views lead each view's
                first-child chain, where iOS looks for what to blur under a
                native header, and nothing fades their native glass away. */}
            <ContentBlurTarget value={androidBlur ? blurTarget : undefined}>
              <ContentBlurReady value={onBlurTargetLayout}>
                <GlassBlurTarget value={androidBlur && blurTargetReady ? blurTarget : undefined}>
                  {children}
                </GlassBlurTarget>
              </ContentBlurReady>
            </ContentBlurTarget>

            {/* Under the native header too, which is transparent over it. */}
            {covered ? null : (
              <GradualBlur
                edge="top"
                height={frameBottom + TOP_FADE}
                color={colors.background}
              />
            )}
            {/* Under a native header the line has no push of its own:
                the screen it belongs to slides in with it. Hidden until the
                header has its height, so it doesn't show at the top first. */}
            {native && chrome?.below ? (
              <View
                onLayout={measureBelow ? (event) => {
                  setBelowHeight(Math.ceil(event.nativeEvent.layout.height));
                } : undefined}
                style={[
                  styles.below,
                  measureBelow && { height: undefined, minHeight: BELOW_HEIGHT },
                  { top: belowTop },
                  nativeHeaderHeight === 0 && styles.unplaced,
                ]}
              >
                {chrome.below}
              </View>
            ) : null}
            {native || covered ? null : (
              <>
                {centerLayers.map((layer) =>
                  layer.chrome.below ? (
                    <HeaderPiece
                      push={push}
                      key={layer.key}
                      // The worktree line follows the header's centre up.
                      stage={layer.incoming ? 1 : 0}
                      incoming={layer.incoming}
                      started={started}
                      still={layer.still}
                      style={[styles.below, { top: belowTop }]}
                    >
                      {layer.chrome.below}
                    </HeaderPiece>
                  ) : null,
              )}
              <View style={[styles.header, { top: headerTop }]}>
                <CircleButton
                  icon="menu"
                  label="Switch project"
                  onPress={() =>
                    router.push({
                      pathname: "/projects",
                      params: projectId ? { current: projectId } : {},
                    })
                  }
                />
                <View style={styles.titleBlock}>
                  {centerLayers.map((layer) => (
                    <HeaderPiece
                      push={push}
                      key={layer.key}
                      stage={0}
                      incoming={layer.incoming}
                      started={started}
                      still={layer.still}
                      glass
                      style={[styles.titlePiece, !layer.incoming && styles.leaving]}
                    >
                      <HeaderTitle chrome={layer.chrome} />
                    </HeaderPiece>
                  ))}
                </View>
                <View>
                  {trailingLayers.map((layer) => (
                    <HeaderPiece
                      push={push}
                      key={layer.key}
                      stage={0}
                      incoming={layer.incoming}
                      started={started}
                      still={layer.still}
                      glass
                      style={!layer.incoming && styles.leaving}
                    >
                      <HeaderTrailing chrome={layer.chrome} />
                    </HeaderPiece>
                  ))}
                </View>
              </View>
              </>
            )}

            {/* Hidden rather than unmounted without a composer, so its measured
              height is still there when the next screen brings one back.
              Full-height, so the composer stays inside it when the keyboard
              lifts it: Fabric drops touches outside a view's laid-out bounds,
              and the lift is a transform that never re-lays the dock out. */}
            <Animated.View
              pointerEvents="box-none"
              style={[
                styles.composerDock,
                (!chrome?.composer || covered) && styles.hidden,
                chrome?.dockStyle,
              ]}
            >
              {/* Its own view, since its lift and a screen's dockStyle each set a transform. */}
              <Animated.View style={keyboardLift}>
                <GradualBlur
                  edge="bottom"
                  height={composerHeight + composerBottom + FADE}
                  color={colors.background}
                />
                <View style={{ paddingBottom: composerBottom }}>
                  <View onLayout={onComposerLayout}>
                    <GlassBlurTarget
                      value={androidBlur && blurTargetReady ? blurTarget : undefined}
                    >
                      {chrome?.composer}
                    </GlassBlurTarget>
                  </View>
                </View>
                {/* Only a transcript needs this glass surface. Empty sessions
                    have no scroll target, so keep it out of the native tree. */}
                {hasJumpTarget ? (
                  <Animated.View
                    style={[
                      styles.jump,
                      { bottom: composerHeight + composerBottom + 10 },
                      jumpStyle,
                    ]}
                  >
                    <LiquidView
                      onPress={() => jumpTarget.current?.()}
                      accessibilityLabel="Scroll to bottom"
                      borderRadius={JUMP / 2}
                      innerStyle={styles.jumpButton}
                    >
                      <Icon name="chevronDown" size={20} />
                    </LiquidView>
                  </Animated.View>
                ) : null}
              </Animated.View>
            </Animated.View>
          </KeyboardGestureArea>
        </SessionLayoutContext>
      </JumpToEndContext>
    </ChromeContext>
  );
}

/**
 * What KeyboardStickyView does, but following the keyboard frame by frame. On
 * iOS, KeyboardStickyView is handed the keyboard's final height once, as it
 * starts to move, and relies on Reanimated applying it inside UIKit's keyboard
 * animation so Core Animation carries it along. Any React commit around focus
 * (the transcript re-rendering as it scrolls, say) delays that update past the
 * animation, and the composer jumps to the top before the keyboard gets there.
 * `onMove` reports the keyboard's position every frame instead, and
 * `onInteractive` follows a drag. `offset` is how much less than the keyboard
 * height to rise, as KeyboardStickyView's `offset.opened`.
 */
export function useKeyboardLift(offset: number) {
  const height = useSharedValue(0);
  const progress = useSharedValue(0);
  const follow = (e: { height: number; progress: number }) => {
    "worklet";
    height.set(e.height);
    progress.set(e.progress);
  };
  useKeyboardHandler(
    { onMove: follow, onInteractive: follow, onEnd: follow },
    [],
  );

  return useAnimatedStyle(() => ({
    transform: [
      {
        translateY:
          -height.get() + interpolate(progress.get(), [0, 1], [0, offset]),
      },
    ],
  }));
}

/**
 * One route's piece of the header in the push between screens: in from below
 * as its route arrives, on up and out as the next one's does. Its glass forms
 * and clears natively alongside, as glass shows through a fading view.
 */
function HeaderPiece({
  push: pushValue,
  stage,
  incoming,
  started,
  still = false,
  glass = false,
  style,
  children,
}: {
  push: SharedValue<number>;
  stage: number;
  incoming: boolean;
  /** Whether the current route's arrival has started. */
  started: boolean;
  /** The same on both routes, so it stays as it is. */
  still?: boolean;
  glass?: boolean;
  style?: StyleProp<ViewStyle>;
  children: ReactNode;
}) {
  const styles = useStyles();
  const push = useHeaderPush(pushValue, stage, incoming, glass, still);
  // An arriving piece is hidden by a plain style until its arrival starts, so
  // it is hidden in the very frame it first draws; the push's animated style
  // only reaches the native view a frame or so after a commit, and the piece
  // would flash in at full before it did. Glass stays just short of clear, so
  // its effect is installed (see useHeaderPush).
  const waiting = incoming && !still && !started;
  // The glass forms as the route's arrival starts, and the last route's
  // clears at that same moment.
  const shown = still || (incoming ? started : !started);
  const presence = useMemo(
    () => ({ shown, duration: GLASS_PUSH_DURATION }),
    [shown],
  );
  return (
    <GlassPresence value={presence}>
      <View
        pointerEvents={incoming ? "box-none" : "none"}
        style={[style, waiting && (glass ? styles.waitingGlass : styles.waiting)]}
      >
        <Animated.View pointerEvents="box-none" style={[styles.pieceInner, push]}>
          {children}
        </Animated.View>
      </View>
    </GlassPresence>
  );
}

/** The header's centre: a screen's own element, its pill, or its title. */
function HeaderTitle({ chrome }: { chrome: Chrome }) {
  const styles = useStyles();
  if (chrome.center) return chrome.center;
  if (chrome.pill)
    return (
      <LiquidView
        style={styles.pillFrame}
        innerStyle={[styles.pill, !chrome.pill.icon && styles.pillTextOnly]}
      >
        {chrome.pill.icon}
        <View style={styles.pillText}>
          <Text
            style={[
              styles.pillLabel,
              chrome.pill.subtitle && styles.pillLabelSmall,
            ]}
            numberOfLines={1}
          >
            {chrome.pill.label}
          </Text>
          {chrome.pill.subtitle ? (
            <Text style={styles.pillSubtitle} numberOfLines={1}>
              {chrome.pill.subtitle}
            </Text>
          ) : null}
        </View>
      </LiquidView>
    );
  if (chrome.title)
    return (
      <>
        <Text style={styles.title} numberOfLines={1}>
          {chrome.title}
        </Text>
        {chrome.subtitle ? (
          <Text style={styles.subtitle} numberOfLines={1}>
            {chrome.subtitle}
          </Text>
        ) : null}
      </>
    );
  return null;
}

/**
 * What the header's right is, so the frame can tell when the next route keeps
 * it: an action by its icon and label, a screen's own element by its route.
 */
function trailingKey(path: string, chrome: Chrome) {
  if (chrome.trailing) return `trailing:${path}`;
  if (chrome.action)
    return `action:${chrome.action.icon}:${chrome.action.label}`;
  return "none";
}

/** Likewise, but one key for every screen sharing a centre, so its button
 *  never remounts as it swaps between theirs. */
function sharedTrailingKey(path: string, chrome: Chrome) {
  return chrome.headerKey
    ? `shared:${chrome.headerKey}`
    : trailingKey(path, chrome);
}

/** A route's centre by its key even when not shared, so it never remounts as it becomes so. */
function centerKey(layer: { path: string; chrome: Chrome }) {
  return layer.chrome.headerKey ? `shared:${layer.chrome.headerKey}` : layer.path;
}

/** The header's right: a screen's own element, its action, or a spacer. */
function HeaderTrailing({ chrome }: { chrome: Chrome }) {
  const styles = useStyles();
  if (chrome.trailing) return chrome.trailing;
  if (chrome.action)
    return (
      <CircleButton
        icon={chrome.action.icon}
        label={chrome.action.label}
        onPress={chrome.action.onPress}
      />
    );
  return <View style={styles.headerSpacer} />;
}

const useStyles = themedStyles((colors) => ({
  scene: {
    flex: 1,
    backgroundColor: colors.background,
  },
  below: {
    position: "absolute",
    left: 0,
    right: 0,
    height: BELOW_HEIGHT,
    alignItems: "center",
    justifyContent: "center",
    paddingHorizontal: 24,
  },
  header: {
    position: "absolute",
    left: 0,
    right: 0,
    height: HEADER_HEIGHT,
    flexDirection: "row",
    alignItems: "center",
    paddingHorizontal: 16,
    gap: 12,
  },
  titleBlock: {
    flex: 1,
    alignItems: "center",
  },
  waiting: {
    opacity: 0,
  },
  waitingGlass: {
    opacity: 0.03,
  },
  pieceInner: {
    maxWidth: "100%",
    alignItems: "center",
  },
  // Centres a title over its subtitle, and lets a long one truncate.
  titlePiece: {
    maxWidth: "100%",
    alignItems: "center",
  },
  // A route's header piece on its way out, over the next one's in its place.
  leaving: {
    position: "absolute",
    top: 0,
    bottom: 0,
    left: 0,
    right: 0,
    alignItems: "center",
    justifyContent: "center",
  },
  title: {
    color: colors.content,
    fontSize: 16,
    fontWeight: "600",
  },
  subtitle: {
    marginTop: 1,
    color: colors.muted,
    fontSize: 12,
  },
  pill: {
    gap: 10,
    height: HEADER_HEIGHT,
    paddingLeft: 12,
    paddingRight: 18,
  },
  // Kept narrow so a long title truncates rather than spanning the header.
  pillFrame: {
    maxWidth: 220,
  },
  pillTextOnly: {
    paddingLeft: 18,
  },
  pillText: {
    flexShrink: 1,
    alignItems: "center",
  },
  pillLabel: {
    color: colors.content,
    fontSize: 17,
    fontWeight: "600",
  },
  pillLabelSmall: {
    fontSize: 15,
  },
  pillSubtitle: {
    marginTop: 1,
    color: colors.muted,
    fontSize: 11,
  },
  headerSpacer: {
    width: 44,
  },
  hidden: {
    display: "none",
  },
  // Kept mounted, so it shows at once when placed.
  unplaced: {
    opacity: 0,
  },
  jump: {
    position: "absolute",
    alignSelf: "center",
  },
  jumpButton: {
    width: JUMP,
    height: JUMP,
    justifyContent: "center",
    alignItems: "center",
  },
  composerDock: {
    position: "absolute",
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    justifyContent: "flex-end",
  },
}));

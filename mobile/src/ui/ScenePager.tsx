import * as Haptics from "expo-haptics";
import { useIsFocused, usePathname } from "expo-router";
import {
  createContext,
  use,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  BackHandler,
  Keyboard,
  Pressable,
  StyleSheet,
  useWindowDimensions,
  View,
} from "react-native";
import { Gesture, GestureDetector } from "react-native-gesture-handler";
import Animated, {
  interpolate,
  ReduceMotion,
  useAnimatedReaction,
  useAnimatedStyle,
  useSharedValue,
  withSpring,
  type SharedValue,
} from "react-native-reanimated";
import { scheduleOnRN } from "react-native-worklets";

import { LinearGradient } from "expo-linear-gradient";

import { useColors, useScheme } from "./theme";

// The Expo Router drawer's motion (react-native-drawer-layout): its spring and
// its swipe thresholds, for the pages and the drawer alike.
const SPRING = {
  stiffness: 1000,
  damping: 500,
  mass: 3,
  overshootClamping: true,
  reduceMotion: ReduceMotion.Never,
};
const SWIPE_MIN_OFFSET = 5;
const SWIPE_MIN_DISTANCE = 60;
const SWIPE_MIN_VELOCITY = 500;
/** How near shut the scene is when a switch from the drawer goes ahead: the
 *  next screen is built only once the motion looks done, so it can't hitch
 *  it. Not at rest: the spring's last few points take as long again. */
const SWITCH_AT = 8;
/** The longest the cover waits for the next screen to arrive. */
const SWAP_TIMEOUT = 1500;
/** Frames the next screen is given to draw under the loading cover. */
const SWITCH_FRAMES = 2;
/** The scene's dimming with the drawer fully open, as that drawer's overlay:
 *  in dark mode only, as a light scene dimmed with black reads as going dark. */
const OVERLAY_OPACITY = { dark: 0.5, light: 0 } as const;
/** The shadow the scene casts on the drawer as it moves aside: a soft band
 *  along its left edge, drawn once and moved with the scene, as a live
 *  shadow on a full-screen view is redrawn each frame and hitches. None in
 *  dark mode, where it reads as a smudge rather than depth. */
const EDGE_SHADOW = { width: 18, dark: "transparent", light: "rgba(0,0,0,0.10)" } as const;
/** What the current drag moves. */
const NONE = 0;
const PAGES = 1;
const DRAWER = 2;

type ScenePager = {
  /** The page index: 0 on the first page, 1 on the second, in between while dragging. */
  progress: SharedValue<number>;
  /** The page the scene is on or springing to. */
  target: SharedValue<number>;
  enabled: SharedValue<boolean>;
  /** How many pages the screen has. */
  pages: SharedValue<number>;
  /** Turns the pan on while a screen with pages is in front, and off
   *  otherwise, so it never holds up the stack's swipe back. */
  setOn: (on: boolean) => void;
};

const ScenePagerContext = createContext<ScenePager | null>(null);

function usePager() {
  const pager = use(ScenePagerContext);
  if (!pager) throw new Error("useScenePager must be used inside ScenePagerHost");
  return pager;
}

/** Dev only: when each stage of a switch from the drawer happens, in ms from
 *  the tap, in the Metro log, to see where a switch's time goes. */
let switchStarted = 0;
export function traceSwitch(stage: string) {
  if (!__DEV__) return;
  if (stage === "tap") switchStarted = performance.now();
  console.log(`[switch] ${Math.round(performance.now() - switchStarted)}ms ${stage}`);
}

type DrawerActions = {
  open: () => void;
  close: () => void;
  /** Shuts the drawer at once, its content covered while it loads (see
   *  useSceneSwitching), and only then runs `change`, a switch to another
   *  screen, showing it once it is drawn. */
  closeThen: (change: () => void) => void;
  /** Lets a swipe right open the drawer while a screen that wants it is in
   *  front; returns the undo. */
  allowSwipe: () => () => void;
};

/** Never changes, so what only opens or closes the drawer doesn't re-render
 *  as it moves. */
const DrawerActionsContext = createContext<DrawerActions | null>(null);
const DrawerOpenContext = createContext(false);

/** The drawer's actions, which never change. */
export function useDrawerActions() {
  const actions = use(DrawerActionsContext);
  if (!actions) throw new Error("useDrawerActions must be used inside ScenePagerHost");
  return actions;
}

/** Whether a switch from the drawer is under way, for the screen's frame to
 *  cover its content, and only that, while it loads. */
const SwitchingContext = createContext({ switching: false, swapping: false });

export function useSceneSwitching() {
  return use(SwitchingContext).switching;
}

/** Whether the switch is swapping one screen for the next right now: the
 *  frame then covers its content whichever screen is going, so the one
 *  leaving never shows through as it is replaced. */
export function useSceneSwapping() {
  return use(SwitchingContext).swapping;
}

/** Whether the drawer is open or opening; re-renders as it opens and closes. */
export function useDrawerOpen() {
  return use(DrawerOpenContext);
}

/**
 * Lets a swipe right open the drawer while this screen is in front, and not
 * once another is pushed or presented over it, so it never holds up the
 * stack's own swipe back.
 */
export function useDrawerSwipe(active = true) {
  const { allowSwipe } = useDrawerActions();
  const focused = useIsFocused();
  useEffect(() => {
    if (focused && active) return allowSwipe();
  }, [focused, active, allowSwipe]);
}

/**
 * Gives the current screen more pages to the right of its first, reached by
 * swiping a page at a time anywhere on the scene. `page` is the screen's state
 * (a tap on a switch, say, animates to it); `onPageChange` reports a swipe
 * that lands on another page. Returns the progress for the screen to slide
 * its pages (and anything that should follow them) by.
 */
export function useScenePager(
  page: number,
  onPageChange: (page: number) => void,
  pageCount = 2,
  /** False on a screen that has no pages after all. */
  active = true,
) {
  const { progress, target, enabled, pages, setOn } = usePager();
  // A screen pushed over this one has no pages; this one's stay where they
  // were for when it comes back.
  const focused = useIsFocused();

  useEffect(() => {
    if (!focused || !active) return;
    enabled.set(true);
    pages.set(pageCount);
    setOn(true);
    return () => {
      enabled.set(false);
      setOn(false);
    };
  }, [focused, active, enabled, pages, pageCount, setOn]);
  useEffect(() => {
    if (!active) return;
    return () => {
      target.set(0);
      progress.set(0);
    };
  }, [active, target, progress]);

  // Report a swipe that lands on another page. A tap's change comes from
  // `page` itself, so reporting it back is a no-op for the screen.
  useAnimatedReaction(
    () => target.get(),
    (next, previous) => {
      if (active && previous !== null && next !== previous) scheduleOnRN(onPageChange, next);
    },
  );

  // A swipe has already set the target and is springing there with the
  // finger's velocity; only a change from React (a tap) starts a new animation.
  useEffect(() => {
    if (!active) return;
    // Later pages have no composer, so the keyboard goes with the first.
    if (page !== 0) Keyboard.dismiss();
    if (target.get() === page) return;
    target.set(page);
    progress.set(withSpring(page, SPRING));
  }, [active, page, target, progress]);

  return progress;
}

/**
 * The pan behind useScenePager, over the whole scene, and the side drawer it
 * also drives, as the Expo Router drawer's "back" type: the scene slides
 * aside, dimming, to uncover `drawer` fixed underneath. Shut, a swipe goes to
 * the pages while there is one that way, and otherwise a deliberate swipe
 * right opens the drawer; open, a swipe left anywhere or a tap on the scene
 * shuts it. One pan for both, so they never
 * fight over a swipe. Nothing but a transform and an opacity moves, so the
 * scene's glass and blur aren't redrawn as it goes.
 */
export function ScenePagerHost({
  children,
  drawer,
}: {
  children: ReactNode;
  drawer?: ReactNode;
}) {
  const colors = useColors();
  const { width } = useWindowDimensions();
  const drawerWidth = Math.min(Math.round(width * 0.82), 330);
  const progress = useSharedValue(0);
  const target = useSharedValue(0);
  const enabled = useSharedValue(false);
  const pages = useSharedValue(2);
  const [on, setOn] = useState(false);
  const [pager] = useState<ScenePager>(() => ({
    progress,
    target,
    enabled,
    pages,
    setOn,
  }));
  /** How far the scene has moved aside for the drawer. */
  const translateX = useSharedValue(0);
  const isOpen = useSharedValue(false);
  // Mirrors isOpen for React: the scene's tap to close, and the drawer's
  // own contents.
  const [open, setOpen] = useState(false);
  /** How many screens in front let a swipe open the drawer: one at a time,
   *  but a switch can mount the next before the last lets go. */
  const swipes = useSharedValue(0);
  const [swipeCount, setSwipeCount] = useState(0);
  /** Where the finger landed, to tell which way it goes. */
  const touchX = useSharedValue(0);
  const touchY = useSharedValue(0);
  /** Where the pages or the scene were when the finger landed, even
   *  mid-animation. */
  const dragStart = useSharedValue(0);
  /** What the current drag moves: NONE, PAGES or DRAWER. */
  const moving = useSharedValue(NONE);

  // Side effects of opening or shutting; the motion itself runs on the UI thread.
  const onToggled = useCallback((willOpen: boolean) => {
    void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light);
    if (willOpen) Keyboard.dismiss();
    setOpen(willOpen);
  }, []);

  const animateTo = useCallback(
    (willOpen: boolean) => {
      if (isOpen.get() !== willOpen) onToggled(willOpen);
      isOpen.set(willOpen);
      translateX.set(withSpring(willOpen ? drawerWidth : 0, SPRING));
    },
    [isOpen, translateX, drawerWidth, onToggled],
  );

  // A switch from the drawer: the cover goes up as it starts to shut, the
  // switch waits for it to be shut, and the cover comes down once the next
  // screen has drawn.
  const [switching, setSwitching] = useState(false);
  const pendingSwitch = useRef<(() => void) | null>(null);
  const switchWaiting = useSharedValue(false);
  // The cover comes down once the next screen has arrived, as the route
  // changing says, and drawn; not a set number of frames after the switch,
  // as a screen can take longer to arrive and the last one shows through.
  const [swapping, setSwapping] = useState(false);
  const pathname = usePathname();
  const route = useRef(pathname);
  const swapFrom = useRef<string | null>(null);
  const finishSwitch = useCallback(() => {
    traceSwitch("new route arrived");
    swapFrom.current = null;
    afterFrames(SWITCH_FRAMES, () => {
      // A newer switch keeps the cover up for its own screen.
      if (pendingSwitch.current || swapFrom.current) return;
      traceSwitch("new screen drawn, cover leaving");
      setSwapping(false);
      setSwitching(false);
    });
  }, []);
  useEffect(() => {
    route.current = pathname;
    if (swapFrom.current !== null && pathname !== swapFrom.current) finishSwitch();
  }, [pathname, finishSwitch]);
  const runSwitch = useCallback(() => {
    const change = pendingSwitch.current;
    pendingSwitch.current = null;
    traceSwitch("drawer shut, switching");
    const from = route.current;
    swapFrom.current = from;
    setSwapping(true);
    change?.();
    traceSwitch("switch called");
    // Nothing arrived (the switch went nowhere): don't hold the cover up.
    setTimeout(() => {
      if (swapFrom.current === from) finishSwitch();
    }, SWAP_TIMEOUT);
  }, [finishSwitch]);
  useAnimatedReaction(
    () => switchWaiting.get() && translateX.get() <= SWITCH_AT,
    (shut, wasShut) => {
      if (!shut || wasShut) return;
      switchWaiting.set(false);
      scheduleOnRN(runSwitch);
    },
  );

  const switchState = useMemo(() => ({ switching, swapping }), [switching, swapping]);

  const actions = useMemo<DrawerActions>(
    () => ({
      open: () => animateTo(true),
      close: () => animateTo(false),
      closeThen: (change) => {
        traceSwitch("tap");
        pendingSwitch.current = change;
        setSwitching(true);
        animateTo(false);
        // Already shut: the cover still draws before the switch.
        if (translateX.get() <= SWITCH_AT) afterFrames(1, runSwitch);
        else switchWaiting.set(true);
      },
      allowSwipe: () => {
        swipes.set(swipes.get() + 1);
        setSwipeCount((count) => count + 1);
        return () => {
          swipes.set(swipes.get() - 1);
          setSwipeCount((count) => count - 1);
        };
      },
    }),
    [animateTo, swipes, translateX, switchWaiting, runSwitch],
  );

  // Android's back shuts the drawer before it leaves the screen.
  useEffect(() => {
    if (!open) return;
    const subscription = BackHandler.addEventListener("hardwareBackPress", () => {
      animateTo(false);
      return true;
    });
    return () => subscription.remove();
  }, [open, animateTo]);

  const hasDrawer = !!drawer;
  // Built once per size: a new gesture each render would be attached anew.
  const pan = useMemo(
    () =>
      Gesture.Pan()
        // Not rebuilt as the drawer opens or shuts: a new gesture would be
        // attached anew mid-animation.
        .enabled(on || (hasDrawer && swipeCount > 0))
        // Takes only a swipe towards a page there is, or the drawer's way, so
        // one off the edge (left on the last page, say) goes to what is under
        // the finger, such as a row's own swipe actions.
        .manualActivation(true)
        .onTouchesDown((event) => {
          const touch = event.allTouches[0];
          touchX.set(touch.absoluteX);
          touchY.set(touch.absoluteY);
        })
        .onTouchesMove((event, manager) => {
          // Decided: the moves after are the drag's own, and judging them
          // again would fail the drawer as soon as the scene moved aside.
          if (moving.get() !== NONE) return;
          const touch = event.allTouches[0];
          const dx = touch.absoluteX - touchX.get();
          const dy = touch.absoluteY - touchY.get();
          if (Math.abs(dy) > SWIPE_MIN_OFFSET && Math.abs(dy) >= Math.abs(dx)) {
            manager.fail();
            return;
          }
          if (Math.abs(dx) <= SWIPE_MIN_OFFSET) return;
          // Open, the drawer takes the swipe that shuts it and nothing else.
          if (isOpen.get() || translateX.get() > 1) {
            if (hasDrawer && dx < 0) {
              moving.set(DRAWER);
              manager.activate();
            } else manager.fail();
            return;
          }
          const next = target.get() + (dx < 0 ? 1 : -1);
          if (enabled.get() && next >= 0 && next < pages.get()) {
            moving.set(PAGES);
            manager.activate();
          } else if (hasDrawer && swipes.get() > 0 && dx > 0) {
            moving.set(DRAWER);
            manager.activate();
          } else manager.fail();
        })
        .onStart(() => {
          dragStart.set(moving.get() === DRAWER ? translateX.get() : progress.get());
        })
        .onUpdate((event) => {
          if (moving.get() === DRAWER) {
            const next = dragStart.get() + event.translationX;
            translateX.set(Math.min(drawerWidth, Math.max(0, next)));
            return;
          }
          if (moving.get() !== PAGES) return;
          const next = dragStart.get() - event.translationX / width;
          // Within a page of where it started, so a long drag can't skip one.
          const from = target.get();
          const last = pages.get() - 1;
          progress.set(
            Math.min(Math.min(last, from + 1), Math.max(Math.max(0, from - 1), next)),
          );
        })
        .onEnd((event) => {
          const { translationX, velocityX } = event;
          // A flick or a long enough drag goes on in its direction; anything
          // else snaps back to where it was.
          const decisive =
            (Math.abs(translationX) > SWIPE_MIN_OFFSET &&
              Math.abs(velocityX) > SWIPE_MIN_VELOCITY) ||
            Math.abs(translationX) > SWIPE_MIN_DISTANCE;
          const leftward = (velocityX === 0 ? translationX : velocityX) < 0;
          if (moving.get() === DRAWER) {
            // The Expo Router drawer's rule, both ways: a flick or a long
            // enough drag goes its way, a nudge springs back.
            const willOpen = decisive ? !leftward : isOpen.get();
            if (isOpen.get() !== willOpen) scheduleOnRN(onToggled, willOpen);
            isOpen.set(willOpen);
            // Springs right here on the UI thread with the finger's velocity,
            // so there is no frame of lag waiting on JS.
            translateX.set(
              withSpring(willOpen ? drawerWidth : 0, { ...SPRING, velocity: velocityX }),
            );
            return;
          }
          if (moving.get() !== PAGES) return;
          const from = target.get();
          const page = decisive
            ? Math.min(pages.get() - 1, Math.max(0, from + (leftward ? 1 : -1)))
            : from;
          if (target.get() !== page) scheduleOnRN(Haptics.selectionAsync);
          target.set(page);
          progress.set(withSpring(page, { ...SPRING, velocity: -velocityX / width }));
        })
        .onFinalize(() => {
          moving.set(NONE);
        }),
    [
      width,
      drawerWidth,
      on,
      hasDrawer,
      swipeCount,
      moving,
      enabled,
      dragStart,
      progress,
      target,
      pages,
      touchX,
      touchY,
      isOpen,
      translateX,
      swipes,
      onToggled,
    ],
  );

  const sceneStyle = useAnimatedStyle(() => ({
    transform: [{ translateX: translateX.get() }],
  }));
  const scheme = useScheme();
  const dim = OVERLAY_OPACITY[scheme];
  // Only there while the scene is aside, so nothing shows at rest.
  const edgeStyle = useAnimatedStyle(() => ({
    opacity: interpolate(translateX.get(), [0, 24], [0, 1], "clamp"),
  }));
  const overlayStyle = useAnimatedStyle(() => ({
    opacity: interpolate(translateX.get(), [0, drawerWidth], [0, dim], "clamp"),
  }));

  return (
    <DrawerActionsContext value={actions}>
      <DrawerOpenContext value={open}>
        <SwitchingContext value={switchState}>
        <ScenePagerContext value={pager}>
          <GestureDetector gesture={pan}>
            <View style={[styles.root, { backgroundColor: colors.drawer }]}>
              {drawer ? (
                // Kept as a view of its own: flattened away, the drawer's
                // header and footer would stack over the scene.
                <View collapsable={false} style={[styles.drawer, { width: drawerWidth }]}>
                  {drawer}
                </View>
              ) : null}
              <Animated.View
                style={[styles.scene, { backgroundColor: colors.background }, sceneStyle]}
              >
                {drawer ? (
                  <Animated.View pointerEvents="none" style={[styles.edge, edgeStyle]}>
                    <LinearGradient
                      colors={["transparent", EDGE_SHADOW[scheme]]}
                      start={{ x: 0, y: 0 }}
                      end={{ x: 1, y: 0 }}
                      style={StyleSheet.absoluteFill}
                    />
                  </Animated.View>
                ) : null}
                {children}
                <Animated.View
                  pointerEvents="none"
                  style={[styles.overlay, overlayStyle]}
                />
                {open ? (
                  <Pressable
                    accessibilityLabel="Close sidebar"
                    style={StyleSheet.absoluteFill}
                    onPress={actions.close}
                  />
                ) : null}
              </Animated.View>
            </View>
          </GestureDetector>
        </ScenePagerContext>
        </SwitchingContext>
      </DrawerOpenContext>
    </DrawerActionsContext>
  );
}

const styles = StyleSheet.create({
  root: {
    flex: 1,
  },
  drawer: {
    position: "absolute",
    top: 0,
    left: 0,
    bottom: 0,
  },
  scene: {
    flex: 1,
  },
  edge: {
    position: "absolute",
    top: 0,
    bottom: 0,
    left: -EDGE_SHADOW.width,
    width: EDGE_SHADOW.width,
  },
  overlay: {
    ...StyleSheet.absoluteFill,
    backgroundColor: "#000",
  },
});

/** Runs `then` once `frames` more frames have been drawn. */
function afterFrames(frames: number, then: () => void) {
  requestAnimationFrame(() => (frames <= 1 ? then() : afterFrames(frames - 1, then)));
}

import * as Haptics from "expo-haptics";
import { useIsFocused } from "expo-router";
import {
  createContext,
  use,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { Keyboard, StyleSheet, useWindowDimensions, View } from "react-native";
import { Gesture, GestureDetector } from "react-native-gesture-handler";
import {
  ReduceMotion,
  useAnimatedReaction,
  useSharedValue,
  withSpring,
  type SharedValue,
} from "react-native-reanimated";
import { scheduleOnRN } from "react-native-worklets";

// The Expo Router drawer's motion (react-native-drawer-layout): its spring and
// its swipe thresholds, as the pages moved when a drawer drove them.
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

/** The pan behind useScenePager, over the whole scene. */
export function ScenePagerHost({ children }: { children: ReactNode }) {
  const { width } = useWindowDimensions();
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
  /** Where the finger landed, to tell which way it goes. */
  const touchX = useSharedValue(0);
  const touchY = useSharedValue(0);
  /** Where the pages were when the finger landed, even mid-animation. */
  const dragStart = useSharedValue(0);
  /** Whether the current drag moves the pages; none on a screen without them. */
  const paging = useSharedValue(false);

  // Built once per width: a new gesture each render would be attached anew.
  const pan = useMemo(
    () =>
      Gesture.Pan()
        .enabled(on)
        // Takes only a swipe towards a page there is, so one off the edge (left
        // on the last page, say) goes to what is under the finger, such as a
        // row's own swipe actions.
        .manualActivation(true)
        .onTouchesDown((event) => {
          const touch = event.allTouches[0];
          touchX.set(touch.absoluteX);
          touchY.set(touch.absoluteY);
        })
        .onTouchesMove((event, manager) => {
          const touch = event.allTouches[0];
          const dx = touch.absoluteX - touchX.get();
          const dy = touch.absoluteY - touchY.get();
          if (Math.abs(dy) > SWIPE_MIN_OFFSET && Math.abs(dy) >= Math.abs(dx)) {
            manager.fail();
            return;
          }
          if (Math.abs(dx) <= SWIPE_MIN_OFFSET) return;
          const next = target.get() + (dx < 0 ? 1 : -1);
          if (enabled.get() && next >= 0 && next < pages.get()) manager.activate();
          else manager.fail();
        })
        .onStart(() => {
          paging.set(enabled.get());
          dragStart.set(progress.get());
        })
        .onUpdate((event) => {
          if (!paging.get()) return;
          const next = dragStart.get() - event.translationX / width;
          // Within a page of where it started, so a long drag can't skip one.
          const from = target.get();
          const last = pages.get() - 1;
          progress.set(
            Math.min(Math.min(last, from + 1), Math.max(Math.max(0, from - 1), next)),
          );
        })
        .onEnd((event) => {
          if (!paging.get()) return;
          const { translationX, velocityX } = event;
          // A flick or a long enough drag turns the page in its direction;
          // anything else snaps back to where it was.
          const decisive =
            (Math.abs(translationX) > SWIPE_MIN_OFFSET &&
              Math.abs(velocityX) > SWIPE_MIN_VELOCITY) ||
            Math.abs(translationX) > SWIPE_MIN_DISTANCE;
          const from = target.get();
          const page = decisive
            ? Math.min(
                pages.get() - 1,
                Math.max(0, from + ((velocityX === 0 ? translationX : velocityX) < 0 ? 1 : -1)),
              )
            : from;
          if (target.get() !== page) scheduleOnRN(Haptics.selectionAsync);
          target.set(page);
          progress.set(withSpring(page, { ...SPRING, velocity: -velocityX / width }));
        }),
    [width, on, paging, enabled, dragStart, progress, target, pages, touchX, touchY],
  );

  return (
    <ScenePagerContext value={pager}>
      <GestureDetector gesture={pan}>
        <View style={styles.root}>{children}</View>
      </GestureDetector>
    </ScenePagerContext>
  );
}

const styles = StyleSheet.create({
  root: {
    flex: 1,
  },
});

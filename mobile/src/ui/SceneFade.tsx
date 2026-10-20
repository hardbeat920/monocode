import { useGlobalSearchParams, usePathname } from "expo-router";
import {
  Easing,
  useAnimatedStyle,
  type SharedValue,
} from "react-native-reanimated";

// The frame's own header, on screens without the stack's native one (a mono):
// as the next screen's header pieces come in from below, the last one's go on
// up and out. Nothing here fades the screens themselves: the stack moves them,
// and native glass drawn first inside a view faded to nothing never forms.

/** How long one header piece takes to push the last screen's out of place. */
const PUSH_DURATION = 300;
/** The worktree line under the header starts this long after the header. */
const PUSH_STAGGER = 50;
/** How far the header pieces travel: in from below, out above. */
const PUSH_DISTANCE = 22;
/** The header's push timeline, long enough for the last piece to finish. */
export const HEADER_PUSH_TOTAL = PUSH_DURATION + PUSH_STAGGER;
const PUSH_EASING = Easing.inOut(Easing.cubic);
/** Never quite clear: the native glass skips its effect while nearly
 *  transparent and adds it with a pop part way through a fade. */
export const GLASS_MIN_OPACITY = 0.03;
/** Native glass forms and clears on its own, as opacity barely touches it. */
export const GLASS_PUSH_DURATION = PUSH_DURATION / 1000;
/** When the last screen's header is gone, so the frame can let it go. */
export const PUSH_DONE_AFTER = HEADER_PUSH_TOTAL + 200;

/** Which screen the main pane shows: its pathname, and its session if any. */
export function useSceneKey() {
  const pathname = usePathname();
  const { sessionId } = useGlobalSearchParams<{ sessionId?: string }>();
  return sessionId ? `${pathname}?session=${sessionId}` : pathname;
}

/** Screens drawn under the stack's own native header, rather than the
 *  frame's: a project and its sessions. */
export const hasNativeHeader = (pathname: string) => pathname.startsWith("/project/");

/** Routes the stack presents over the screen as native modals, which leave
 *  the screen under them as it was. */
const MODAL_ROUTES = ["/projects", "/settings", "/models", "/changes", "/file", "/diff"];
export const isModalRoute = (pathname: string) =>
  MODAL_ROUTES.some((route) => pathname === route || pathname.startsWith(`${route}/`));

/**
 * A header piece in the push: the arriving screen's come up from below into
 * place (`incoming`), the last screen's go on up and out. `stage` 1 starts a
 * beat behind 0. Set `glass` where it holds native glass, which must never go
 * fully clear, so its effect is installed up front; the glass itself forms and
 * clears by GlassPresence.
 */
export function useHeaderPush(
  /** Milliseconds into the header's push, run by the frame. */
  push: SharedValue<number>,
  stage: number,
  incoming: boolean,
  glass = false,
  /** Stays put, as when the next screen has the same piece. */
  still = false,
) {
  return useAnimatedStyle(() => {
    if (still) return { opacity: 1, transform: [{ translateY: 0 }] };
    const t = PUSH_EASING(
      Math.min(
        1,
        Math.max(0, (push.get() - stage * PUSH_STAGGER) / PUSH_DURATION),
      ),
    );
    const shown = incoming ? t : 1 - t;
    return {
      // Arriving glass is installed up front; leaving glass already is, so
      // it may go fully clear.
      opacity: glass && incoming ? Math.max(GLASS_MIN_OPACITY, shown) : shown,
      transform: [
        { translateY: incoming ? (1 - t) * PUSH_DISTANCE : -t * PUSH_DISTANCE },
      ],
    };
  });
}

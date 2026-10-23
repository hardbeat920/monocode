import type { ReactNode } from "react";
import { Image, ScrollView, StyleSheet, Text, View } from "react-native";
import { useReanimatedKeyboardAnimation } from "react-native-keyboard-controller";
import Animated, {
  Easing,
  FadeIn,
  useAnimatedStyle,
} from "react-native-reanimated";

import { AsciiField } from "@/features/background/AsciiField";
import { FadingBackground } from "@/features/background/FadingBackground";
import type { EmptyBackground } from "@/features/background/useEmptyBackground";
import type { Project } from "@/features/workspace/types";
import { MONOCODE_MARK } from "@/ui/brand";
import { themedStyles } from "@/ui/theme";

import { COMPOSER_FADE, useSessionLayout } from "./SessionScreen";

export const EMPTY_HEADLINE = "What should we work on?";

const MARK_SIZE = 84;
/** Dimmed, so it reads as part of the screen, including over a photo. */
const MARK_OPACITY = 0.3;

/** Space between the composer and what rests on it. */
const ACCESSORY_GAP = 10;

/** The photo is drawn a moment after the screen opens, so it eases in slower. */
const BACKGROUND_ENTER = FadeIn.duration(520).easing(Easing.out(Easing.quad));

// Empty conversation: a dimmed Monocode logo above the prompt, and the
// project's name under it, over the background set for it on this phone, or
// a still frame of the connect screen's ASCII field where no photo is set.
export function EmptySession({
  project,
  background,
  hasPhoto = false,
  accessory,
}: {
  project?: Project;
  /** From useEmptyBackground; none until a photo is picked and drawn. */
  background?: EmptyBackground | null;
  /** Whether a photo is set, drawn yet or not; without one, the ASCII field. */
  hasPhoto?: boolean;
  /** Rests on top of the composer, as the working sessions' pill. */
  accessory?: ReactNode;
}) {
  const styles = useStyles();
  // Held hidden until the insets it centres between are final, so it doesn't
  // jump up as the composer is first measured.
  const { top, bottom, settled, extraContentPadding } = useSessionLayout();
  // The logo and headline fade out as the keyboard opens, so the raised composer
  // doesn't sit on top of them, and back as it closes or is dragged away.
  const keyboard = useReanimatedKeyboardAnimation();
  const fade = useAnimatedStyle(() => ({
    opacity: 1 - keyboard.progress.get(),
  }));
  // Rides the composer's top edge as it grows past one line.
  const lift = useAnimatedStyle(() => ({
    transform: [{ translateY: -(extraContentPadding?.get() ?? 0) }],
  }));
  return (
    <View style={styles.root}>
      {background ? (
        <Animated.View
          pointerEvents="none"
          style={StyleSheet.absoluteFill}
          entering={BACKGROUND_ENTER}
        >
          <FadingBackground {...background} />
        </Animated.View>
      ) : hasPhoto ? null : (
        <Animated.View
          pointerEvents="none"
          style={StyleSheet.absoluteFill}
          entering={BACKGROUND_ENTER}
        >
          <AsciiField still />
        </Animated.View>
      )}
      {/* A scroll view with nothing to scroll, as in the transcript: it bounces,
          so dragging down takes the keyboard with it, and a tap dismisses it. */}
      <ScrollView
        alwaysBounceVertical
        keyboardDismissMode="interactive"
        showsVerticalScrollIndicator={false}
        style={!settled && styles.unsettled}
        contentContainerStyle={[
          styles.container,
          { paddingTop: top, paddingBottom: bottom },
        ]}
      >
        <Animated.View style={[styles.intro, fade]}>
          <Image
            source={MONOCODE_MARK}
            style={styles.mark}
            accessibilityIgnoresInvertColors
          />
          <Text style={styles.headline}>{EMPTY_HEADLINE}</Text>
          {project ? (
            <Text style={styles.projectName} numberOfLines={1}>
              {project.name}
            </Text>
          ) : null}
        </Animated.View>
      </ScrollView>
      {accessory ? (
        <Animated.View
          pointerEvents="box-none"
          style={[
            styles.accessory,
            // `bottom` reaches past the composer by its fade.
            { bottom: bottom - COMPOSER_FADE + ACCESSORY_GAP },
            !settled && styles.unsettled,
            lift,
          ]}
        >
          {accessory}
        </Animated.View>
      ) : null}
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  root: {
    flex: 1,
  },
  container: {
    flexGrow: 1,
    alignItems: "center",
    justifyContent: "center",
    paddingHorizontal: 32,
  },
  intro: {
    alignItems: "center",
    gap: 18,
    // Lifts the group so it sits closer to the true centre.
    paddingBottom: 110,
  },
  accessory: {
    position: "absolute",
    left: 0,
    right: 0,
  },
  unsettled: {
    opacity: 0,
  },
  mark: {
    width: MARK_SIZE,
    height: MARK_SIZE,
    opacity: MARK_OPACITY,
    tintColor: colors.mark,
  },
  headline: {
    color: colors.content,
    fontSize: 22,
    fontWeight: "600",
    textAlign: "center",
    lineHeight: 28,
  },
  // Close under the headline, as its caption.
  projectName: {
    maxWidth: "100%",
    marginTop: -8,
    color: colors.muted,
    fontSize: 15,
    fontWeight: "500",
  },
}));

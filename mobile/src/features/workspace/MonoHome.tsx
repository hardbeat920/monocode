import * as Haptics from "expo-haptics";
import { Link, router, Stack, useIsFocused, useNavigation } from "expo-router";
import { memo, useEffect, useMemo, useRef, useState } from "react";
import { Alert, Image, Pressable, ScrollView, StyleSheet, Text, TextInput, View } from "react-native";
import Animated, {
  FadeIn,
  FadeOut,
  LayoutAnimationConfig,
  LinearTransition,
} from "react-native-reanimated";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { useConnection, useMonoTools } from "@/features/connection/ConnectionProvider";
import { hideScrollEdgeEffects } from "@/features/connection/nativeLink";
import { PixelMascot } from "@/features/mascots/PixelMascot";
import { HeaderBlur } from "@/features/files/codeScreen";
import { plainText } from "@/features/sessions/markdownParser";
import { useKeyboardLift } from "@/features/sessions/SessionScreen";
import { MONOCODE_MARK } from "@/ui/brand";
import { CircleButton } from "@/ui/CircleButton";
import { GradualBlur } from "@/ui/GradualBlur";
import { Icon } from "@/ui/Icon";
import { LiquidView } from "@/ui/LiquidView";
import { Shimmer } from "@/ui/Shimmer";
import { themedStyles, useColors, useScheme } from "@/ui/theme";

import { markMonoSeen, markMonoUnread, useUnreadMonos } from "./monoSeen";
import { onMonoCreated } from "./newMono";
import type { Mono } from "./types";
import { oneLine, when } from "./WorkspaceDrawer";
import { useWorkspace } from "./WorkspaceProvider";

/** The list's tag, for turning off iOS's edge effect on it. */
const LIST_ID = "mono-list";
/** As the composer's pill, so the list's foot lines up with a chat's. */
const FOOTER_HEIGHT = 50;
/** How far past the header and footer the blur keeps fading. */
const FADE = 16;
/** A Mono's mascot, as a messaging app's avatar. */
const AVATAR = 44;
/** The mascot itself, a little smaller, its left edge level with the mark's. */
const MASCOT = 36;
/** How long rows take to glide to new places, and a leaving one to fade. */
const MOVE_MS = 280;
const FADE_OUT_MS = 180;
/** A long press's preview of a Mono's latest message: iOS's preferred size. */
const PREVIEW_WIDTH = 340;
const PREVIEW_HEIGHT = 260;

/**
 * The Monos-only layout's first screen, as a messaging app's chat list:
 * the Monocode mark and a new Mono in the native header, the Monos heard
 * from last first, and settings and search in glass at the foot, level with a chat's
 * composer. A Mono's chat pushes over it.
 */
export function MonoHome() {
  const insets = useSafeAreaInsets();
  const colors = useColors();
  const styles = useStyles();
  const { monos: roster } = useWorkspace();
  const { pairing, command, refresh, selection, select } = useConnection();
  const tools = useMonoTools();
  const [query, setQuery] = useState("");
  const [listReady, setListReady] = useState(false);
  // iOS's edge effect, a band with a hard line, off under the header once
  // the list is in: the stack misses this list, so it's done directly.
  useEffect(() => {
    if (!listReady) return;
    hideScrollEdgeEffects(LIST_ID);
    // Again a moment later, in case the stack set it back meanwhile.
    const timer = setTimeout(() => hideScrollEdgeEffects(LIST_ID), 500);
    return () => clearTimeout(timer);
  }, [listReady]);
  // A Mono made in the new-Mono sheet opens once the sheet is gone and the
  // list is in front again, so its push doesn't meet the sheet's dismissal.
  const created = useRef<string | null>(null);
  useEffect(
    () =>
      onMonoCreated((monoId) => {
        created.current = monoId;
      }),
    [],
  );
  const unread = useUnreadMonos(pairing?.desktop.fingerprint, roster);
  // Its rows' shimmer runs only while the list is in front.
  const focused = useIsFocused();
  // Whether a chat has finished sliding over the list; it is uncovered again
  // as soon as the list is back in front.
  const navigation = useNavigation();
  const [coveredByChat, setCovered] = useState(false);
  useEffect(
    () =>
      // As the chat starts to slide over: iOS takes the list off screen the
      // moment the push ends, and with the blurs still on it that stalls.
      navigation.addListener("transitionStart" as never, (event: { data?: { closing?: boolean } }) => {
        if (event.data?.closing) setCovered(true);
      }),
    [navigation],
  );
  const covered = coveredByChat && !focused;
  if (coveredByChat && focused) setCovered(false);

  // Monos deleted or hidden here leave the list at once, before the desktop
  // has answered; one it refuses comes back.
  const [gone, setGone] = useState<ReadonlySet<string>>(() => new Set());
  const setGoneId = (id: string, isGone: boolean) =>
    setGone((previous) => {
      const next = new Set(previous);
      if (isGone) next.add(id);
      else next.delete(id);
      return next;
    });

  const needle = query.trim().toLowerCase();
  const monos = useMemo(
    () =>
      [...roster]
        .sort((a, b) => (b.lastActiveAt || 0) - (a.lastActiveAt || 0))
        .filter(
          (mono) =>
            !gone.has(mono.id) && (!needle || mono.name.toLowerCase().includes(needle)),
        ),
    [roster, needle, gone],
  );

  useEffect(() => {
    const monoId = created.current;
    if (!focused || !monoId) return;
    created.current = null;
    openMono(monoId);
  }, [focused]);

  // A row's long-press menu. Reset and delete clear what can't come back, so
  // each asks first, as the desktop does.
  const desktop = pairing?.desktop.fingerprint;
  const act = async (run: () => Promise<unknown>) => {
    try {
      await run();
      void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
      await refresh();
    } catch (reason) {
      Alert.alert("Couldn’t do that", reason instanceof Error ? reason.message : String(reason));
    }
  };
  // Out of the list now, and back if the desktop refuses.
  const removeNow = (mono: Mono, run: () => Promise<unknown>) => {
    void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
    setGoneId(mono.id, true);
    run()
      .then(() => refresh())
      .catch((reason: unknown) => {
        setGoneId(mono.id, false);
        Alert.alert("Couldn’t do that", reason instanceof Error ? reason.message : String(reason));
      });
  };
  const handlers: RowActions = {
    toggleRead: (mono) =>
      unread.has(mono.id) ? markMonoSeen(desktop, mono) : markMonoUnread(desktop, mono),
    details: tools
      ? (mono) => router.push({ pathname: "/mono-profile/[monoId]", params: { monoId: mono.id } })
      : undefined,
    reset: tools
      ? (mono) =>
          Alert.alert(
            "Reset conversation?",
            `The chat with ${mono.name} will be cleared. Its soul, memory and habits stay.`,
            [
              { text: "Cancel", style: "cancel" },
              {
                text: "Reset",
                style: "destructive",
                onPress: () => void act(() => command("mono.reset", { monoId: mono.id })),
              },
            ],
          )
      : undefined,
    // Off this phone only; it comes back from Workspace in settings.
    hide: selection
      ? (mono) =>
          removeNow(mono, () =>
            select({ ...selection, monoIds: selection.monoIds.filter((id) => id !== mono.id) }),
          )
      : undefined,
    remove: tools
      ? (mono) =>
          Alert.alert(
            `Delete ${mono.name}?`,
            "Its scheduled habits stop. Sessions and notes it created stay saved.",
            [
              { text: "Cancel", style: "cancel" },
              {
                text: "Delete",
                style: "destructive",
                onPress: () => removeNow(mono, () => command("mono.delete", { monoId: mono.id })),
              },
            ],
          )
      : undefined,
  };
  const latest = useRef<RowActions | null>(null);
  useEffect(() => {
    latest.current = handlers;
  });
  // Stable, so the memoized rows don't re-render for it; each call reads the
  // latest handlers.
  const [actions] = useState<RowActions>(() => ({
    toggleRead: (mono) => latest.current?.toggleRead(mono),
    details: (mono) => latest.current?.details?.(mono),
    reset: (mono) => latest.current?.reset?.(mono),
    hide: (mono) => latest.current?.hide?.(mono),
    remove: (mono) => latest.current?.remove?.(mono),
  }));

  const footerBottom = Math.max(insets.bottom, 12);
  const footerTop = footerBottom + FOOTER_HEIGHT + 12;
  // The search rides its own keyboard, resting just above it.
  const search = useRef<TextInput>(null);
  const lift = useKeyboardLift(footerBottom - 10, search);
  const scheme = useScheme();

  return (
    <View style={styles.container}>
      {/* First, so it leads the screen's first-child chain: that is where
          iOS looks for the scroll view whose edge effect is turned off
          under the header, leaving our own gradual blur. */}
      <ScrollView
        testID={LIST_ID}
        onLayout={listReady ? undefined : () => setListReady(true)}
        onScrollBeginDrag={() => hideScrollEdgeEffects(LIST_ID)}
        style={StyleSheet.absoluteFill}
        // Clear of the native header, which it runs under, and of the foot.
        contentInsetAdjustmentBehavior="automatic"
        contentContainerStyle={{ paddingTop: 4, paddingBottom: footerTop + FADE }}
        keyboardShouldPersistTaps="handled"
        keyboardDismissMode="on-drag"
      >
        {/* Rows glide to their new places as one leaves or a new message
            brings one to the top, and fade as they go or come; the first
            ones in are simply there. */}
        <LayoutAnimationConfig skipEntering>
          {monos.map((mono) => (
            <Animated.View
              key={mono.id}
              layout={LinearTransition.duration(MOVE_MS)}
              entering={FadeIn.duration(MOVE_MS)}
              exiting={FadeOut.duration(FADE_OUT_MS)}
            >
              <MonoRow
                mono={mono}
                unread={unread.has(mono.id)}
                shown={focused}
                tools={tools}
                actions={actions}
              />
            </Animated.View>
          ))}
        </LayoutAnimationConfig>
        {monos.length ? null : (
          <View style={styles.empty}>
            <Text style={styles.emptyText}>
              {needle ? "Nothing matches." : "No Monos on this phone yet."}
            </Text>
            {needle ? null : (
              <Text style={styles.emptyLink} onPress={() => router.push("/settings/workspace")}>
                Choose Monos
              </Text>
            )}
          </View>
        )}
      </ScrollView>

      {/* The stack's native header: the Monocode mark and a new Mono as
          native items, so they morph into a chat's back and ⋯ as one is
          pushed. */}
      <Stack.Screen options={{ headerTitle: "" }} />
      <Stack.Toolbar placement="left">
        <Stack.Toolbar.View hidesSharedBackground>
          <View style={styles.markBox}>
            <Image
              source={MONOCODE_MARK}
              style={[styles.mark, { tintColor: colors.content }]}
              accessibilityLabel="Monocode"
              accessibilityIgnoresInvertColors
            />
          </View>
        </Stack.Toolbar.View>
      </Stack.Toolbar>
      {/* The older desktop app can't make a Mono from the phone. */}
      {tools ? (
        <Stack.Toolbar placement="right">
          <Stack.Toolbar.Button
            icon="plus"
            accessibilityLabel="New Mono"
            onPress={() => router.push("/mono-new")}
          />
        </Stack.Toolbar>
      ) : null}

      {/* The app's own gradual blur under the header and the foot, as
          over every screen, rather than iOS's edge effects. Not kept while
          a chat covers the list: its masked layers are rebuilt each time
          iOS takes the list off screen or puts it back, which stalls the
          slides in and out of a chat. */}
      {covered ? null : (
        <>
          <HeaderBlur color={colors.background} />
          <GradualBlur edge="bottom" height={footerTop + FADE} color={colors.background} />
        </>
      )}

      <Animated.View style={[styles.foot, { bottom: footerBottom }, lift]}>
        <View style={styles.footer}>
          <CircleButton
            icon="settings"
            label="Settings"
            onPress={() => router.push("/settings")}
            size={FOOTER_HEIGHT}
          />
          <LiquidView
            borderRadius={FOOTER_HEIGHT / 2}
            style={styles.searchFrame}
            innerStyle={styles.search}
          >
            <Icon name="search" size={18} color={colors.muted} />
            <TextInput
              ref={search}
              value={query}
              onChangeText={setQuery}
              placeholder="Search"
              placeholderTextColor={colors.muted}
              keyboardAppearance={scheme}
              returnKeyType="search"
              autoCorrect={false}
              clearButtonMode="while-editing"
              style={styles.searchInput}
            />
          </LiquidView>
        </View>
      </Animated.View>
    </View>
  );
}

/** Pushes a Mono's chat over the list. */
function openMono(id: string) {
  router.push({ pathname: "/mono/[monoId]", params: { monoId: id } });
}

type RowActions = {
  toggleRead: (mono: Mono) => void;
  details?: (mono: Mono) => void;
  reset?: (mono: Mono) => void;
  hide?: (mono: Mono) => void;
  remove?: (mono: Mono) => void;
};

/** A Mono as a chat in the list: its mascot, its name and when it was last
 *  active, and under them what it is doing or its last line. A tap opens its
 *  chat; a long press lifts it into iOS's own preview of its latest message,
 *  with a menu of what can be done to it, as Messages does. */
const MonoRow = memo(function MonoRow({
  mono,
  unread,
  shown,
  tools,
  actions,
}: {
  mono: Mono;
  /** A reply it sent that the user hasn't seen. */
  unread: boolean;
  /** Whether the list is in front, for the working shimmer. */
  shown: boolean;
  /** Whether the desktop can show its details, reset or delete it. */
  tools: boolean;
  actions: RowActions;
}) {
  const styles = useStyles();
  const detail =
    mono.status === "working"
      ? "Working…"
      : mono.status === "needs-you"
        ? "Needs you"
        : oneLine(mono.lastMessage) || "No messages yet";
  return (
    <Link
      href={{ pathname: "/mono/[monoId]", params: { monoId: mono.id } }}
      asChild
      onPress={() => {
        void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light);
      }}
    >
      <Link.Trigger>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={`${mono.name}, ${detail}`}
          style={styles.row}
        >
          <PixelMascot name={mono.mascot} color={mono.color} status={mono.status} size={MASCOT} centered />
          <View style={styles.rowText}>
            <View style={styles.rowLine}>
              <Text style={styles.rowLabel} numberOfLines={1}>
                {mono.name}
              </Text>
              {mono.lastActiveAt ? (
                <Text style={[styles.rowTime, unread && styles.rowTimeUnread]}>
                  {when(mono.lastActiveAt)}
                </Text>
              ) : null}
            </View>
            <View style={styles.rowLine}>
              {mono.status === "working" && shown ? (
                <Shimmer style={[styles.rowDetail, styles.rowFill]}>{detail}</Shimmer>
              ) : (
                <Text
                  style={[
                    styles.rowDetail,
                    styles.rowFill,
                    mono.status === "needs-you" && styles.rowNeedsYou,
                  ]}
                  numberOfLines={1}
                >
                  {detail}
                </Text>
              )}
              {unread ? <View style={styles.unread} /> : null}
            </View>
          </View>
        </Pressable>
      </Link.Trigger>
      <Link.Preview style={styles.preview}>
        <MonoPreview mono={mono} />
      </Link.Preview>
      <Link.Menu>
        <Link.MenuAction
          icon={unread ? "message" : "message.badge"}
          onPress={() => actions.toggleRead(mono)}
        >
          {unread ? "Mark as Read" : "Mark as Unread"}
        </Link.MenuAction>
        {tools ? (
          <Link.MenuAction icon="info.circle" onPress={() => actions.details?.(mono)}>
            View Details
          </Link.MenuAction>
        ) : null}
        {tools ? (
          <Link.MenuAction icon="arrow.counterclockwise" onPress={() => actions.reset?.(mono)}>
            Reset Conversation
          </Link.MenuAction>
        ) : null}
        <Link.MenuAction icon="eye.slash" onPress={() => actions.hide?.(mono)}>
          Hide
        </Link.MenuAction>
        {tools ? (
          <Link.MenuAction icon="trash" destructive onPress={() => actions.remove?.(mono)}>
            Delete Mono
          </Link.MenuAction>
        ) : null}
      </Link.Menu>
    </Link>
  );
});

/** What a long press lifts: the Mono's name over its latest message, in a
 *  bubble as its chat shows it. */
function MonoPreview({ mono }: { mono: Mono }) {
  const styles = useStyles();
  const text =
    mono.status === "working"
      ? "Working…"
      : plainText(mono.lastMessage.slice(0, 1200)).trim() || "No messages yet";
  return (
    <View style={styles.previewCard}>
      <View style={styles.previewHeader}>
        <PixelMascot name={mono.mascot} color={mono.color} status={mono.status} size={28} centered />
        <Text style={styles.previewName} numberOfLines={1}>
          {mono.name}
        </Text>
      </View>
      <View style={styles.previewBody}>
        <View style={styles.bubble}>
          <Text style={styles.bubbleText} numberOfLines={8}>
            {text}
          </Text>
        </View>
      </View>
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  container: {
    flex: 1,
    backgroundColor: colors.background,
  },
  preview: {
    width: PREVIEW_WIDTH,
    height: PREVIEW_HEIGHT,
  },
  previewCard: {
    flex: 1,
    backgroundColor: colors.background,
  },
  previewHeader: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
    paddingHorizontal: 18,
    paddingVertical: 14,
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: colors.stroke,
  },
  previewName: {
    flex: 1,
    color: colors.content,
    fontSize: 17,
    fontWeight: "600",
  },
  previewBody: {
    flex: 1,
    padding: 16,
  },
  bubble: {
    alignSelf: "flex-start",
    maxWidth: "88%",
    paddingHorizontal: 16,
    paddingVertical: 12,
    borderRadius: 22,
    borderCurve: "continuous",
    backgroundColor: colors.surface,
  },
  bubbleText: {
    color: colors.content,
    fontSize: 16,
    lineHeight: 22,
  },
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    minHeight: 70,
    // The mascot's left edge level with the Monocode mark's: iOS insets
    // the bar's items 16pt, and the mark sits 3pt into its 44pt box.
    paddingLeft: 19,
    paddingRight: 20,
    paddingVertical: 12,
  },
  rowText: {
    flex: 1,
    minWidth: 0,
    gap: 4,
  },
  rowLine: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  rowLabel: {
    flex: 1,
    color: colors.content,
    fontSize: 16,
    fontWeight: "500",
  },
  rowTime: {
    color: colors.muted,
    fontSize: 13,
    fontVariant: ["tabular-nums"],
  },
  rowTimeUnread: {
    color: colors.accent,
  },
  rowDetail: {
    color: colors.muted,
    fontSize: 14,
  },
  rowFill: {
    flex: 1,
  },
  rowNeedsYou: {
    color: colors.caution,
  },
  unread: {
    width: 10,
    height: 10,
    borderRadius: 5,
    backgroundColor: colors.accent,
  },
  empty: {
    paddingHorizontal: 20,
    paddingTop: 12,
    gap: 8,
  },
  emptyText: {
    color: colors.muted,
    fontSize: 15,
  },
  // The Monocode mark, centred over the rows' mascots below it.
  markBox: {
    width: AVATAR,
    height: AVATAR,
    alignItems: "center",
    justifyContent: "center",
  },
  mark: {
    width: 38,
    height: 38,
  },
  emptyLink: {
    color: colors.accent,
    fontSize: 15,
  },
  foot: {
    position: "absolute",
    left: 0,
    right: 0,
  },
  footer: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    paddingHorizontal: 16,
  },
  searchFrame: {
    flex: 1,
  },
  search: {
    height: FOOTER_HEIGHT,
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    paddingHorizontal: 16,
  },
  searchInput: {
    flex: 1,
    height: FOOTER_HEIGHT,
    color: colors.content,
    fontSize: 17,
  },
}));

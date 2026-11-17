import { useIsFocused } from "expo-router";
import { useEffect, useRef, useState } from "react";
import { ActivityIndicator, Alert, Image, Pressable, Text, View } from "react-native";
import { WebView, type WebViewMessageEvent } from "react-native-webview";
import type { ShouldStartLoadRequest } from "react-native-webview/lib/WebViewTypes";

import { useConnection } from "@/features/connection/ConnectionProvider";
import { makeCommandId } from "@/features/connection/nativeLink";
import { Icon } from "@/ui/Icon";
import { useKeepsSwipe } from "@/ui/ScenePager";
import { radii, themedStyles, useColors, useScheme, withAlpha } from "@/ui/theme";

import {
  DraftController,
  type DraftCardItem,
  type DraftChrome,
  type DraftPress,
  type DraftSurface,
} from "./DraftController";
import { parseMessage } from "./sync";

const ESTIMATE = 280;
const TALLEST = 12000;
/** A finger that travels this far up or down is scrolling the thread. */
const SCROLL_SLOP = 10;

// The page and its sandboxed frame load from memory; nothing navigates.
const allow = (request: ShouldStartLoadRequest) => request.url.startsWith("about:");

/**
 * A draft in the thread where the transcript is React: Android's. The same
 * card as iOS's, from the same controller, without the full view.
 */
export function DraftCard({
  conversationId,
  card,
}: {
  conversationId: string;
  card: DraftCardItem;
}) {
  const { command } = useConnection();
  const light = useScheme() === "light";
  const colors = useColors();
  const styles = useStyles();
  const focused = useIsFocused();
  const keepsSwipe = useKeepsSwipe();
  const [chrome, setChrome] = useState<DraftChrome | null>(null);
  const [page, setPage] = useState<{ html: string; generation: number } | null>(null);
  const [picture, setPicture] = useState<{ uri: string; ratio?: number } | null>(null);
  const [height, setHeight] = useState<number | null>(null);
  const web = useRef<WebView>(null);
  const controller = useRef<DraftController | null>(null);
  const touch = useRef({ at: -Infinity, before: -Infinity, x: 0, y: 0 });

  useEffect(() => {
    const surface: DraftSurface = {
      chrome: setChrome,
      load: (html, generation) => setPage({ html, generation }),
      script: (script) => web.current?.injectJavaScript(script),
      picture: (uri, size) =>
        setPicture(uri ? { uri, ratio: size && size.height / size.width } : null),
    };
    const next = new DraftController({
      conversationId,
      card,
      light,
      command,
      commandId: makeCommandId,
      surface,
      alert: (title, message) => Alert.alert(title, message),
    });
    controller.current = next;
    return () => {
      next.dispose();
      controller.current = null;
    };
    // One controller per card; `setCard` keeps it current.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [conversationId, card.draft]);

  useEffect(() => controller.current?.setCard(card), [card]);
  useEffect(() => {
    if (focused) controller.current?.show();
    else controller.current?.hide();
  }, [conversationId, card.draft, focused]);

  const press = (button: DraftPress) => controller.current?.press(button);
  const said = (event: WebViewMessageEvent) => {
    const { data } = event.nativeEvent;
    const message = parseMessage(data);
    if (message?.k === "h") {
      setHeight(Math.min(TALLEST, Math.ceil(message.v)));
      return;
    }
    controller.current?.message(data, touch.current.at);
  };

  const mode = chrome?.mode ?? "loading";
  const notes = chrome?.notes ?? 0;
  const title = chrome?.title ?? card.title;
  if (mode === "closed")
    return (
      <View style={[styles.card, styles.closed]}>
        <Text style={styles.status} numberOfLines={1}>
          {chrome?.status}
        </Text>
        {chrome?.open ? <Pill label={chrome.open} onPress={() => press("open")} /> : null}
      </View>
    );
  const pictured = mode === "frame";
  return (
    <View style={styles.card}>
      <View style={styles.header}>
        <Text style={styles.title} numberOfLines={1}>
          {title}
        </Text>
        {mode === "page" ? (
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Comment"
            accessibilityState={{ selected: !!chrome?.commenting }}
            onPress={() => press("comment")}
            style={styles.icon}
          >
            <Icon
              name="comment"
              size={16}
              color={chrome?.commenting ? colors.accent : colors.muted}
            />
          </Pressable>
        ) : null}
        {mode === "page" || mode === "frame" || mode === "paused" ? (
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Close draft"
            onPress={() => press("close")}
            style={styles.icon}
          >
            <Icon name="close" size={16} color={colors.muted} />
          </Pressable>
        ) : null}
      </View>
      {mode === "paused" ? (
        <View style={styles.paused}>
          {chrome?.open ? (
            <Pill label={chrome.open} onPress={() => press("open")} />
          ) : null}
        </View>
      ) : pictured && picture ? (
        <Image
          source={{ uri: picture.uri }}
          style={{ width: "100%", aspectRatio: picture.ratio ? 1 / picture.ratio : 1.4 }}
          resizeMode="cover"
        />
      ) : mode === "page" && page ? (
        <View
          style={{ height: height ?? chrome?.height ?? ESTIMATE }}
          onTouchEnd={keepsSwipe.onTouchEnd}
          onTouchCancel={keepsSwipe.onTouchCancel}
          onTouchStart={({ nativeEvent }) => {
            keepsSwipe.onTouchStart();
            const now = touch.current;
            touch.current = {
              at: Date.now(),
              before: now.at,
              x: nativeEvent.pageX,
              y: nativeEvent.pageY,
            };
          }}
          onTouchMove={({ nativeEvent }) => {
            const now = touch.current;
            const up = Math.abs(nativeEvent.pageY - now.y);
            const across = Math.abs(nativeEvent.pageX - now.x);
            // The thread took it: that was a scroll, not a use of the page.
            if (up > SCROLL_SLOP && up > across) {
              if (now.at !== now.before) controller.current?.touchTaken();
              now.at = now.before;
            } else now.at = Date.now();
          }}
        >
          <WebView
            ref={web}
            key={page.generation}
            source={{ html: page.html }}
            originWhitelist={["about:*"]}
            onShouldStartLoadWithRequest={allow}
            onLoadStart={() => controller.current?.loadStarted()}
            onLoadEnd={() => controller.current?.loaded(page.generation)}
            onMessage={said}
            scrollEnabled={false}
            nestedScrollEnabled
            incognito
            style={styles.web}
          />
        </View>
      ) : (
        <View style={styles.loading}>
          <ActivityIndicator color={colors.muted} />
        </View>
      )}
      {chrome?.status || notes > 0 || chrome?.handBack || pictured ? (
        <View style={styles.footer}>
          {chrome?.waiting ? <View style={styles.dot} /> : null}
          <Text style={styles.status} numberOfLines={1}>
            {chrome?.status}
          </Text>
          {chrome?.open && pictured ? (
            <Pill label={chrome.open} onPress={() => press("open")} />
          ) : null}
          {notes > 0 ? (
            <Pill
              label={`Send ${notes} ${notes === 1 ? "note" : "notes"}`}
              strong
              onPress={() => press("send")}
            />
          ) : null}
          {chrome?.handBack ? (
            <Pill label="Hand back" onPress={() => press("handBack")} />
          ) : null}
        </View>
      ) : null}
    </View>
  );
}

function Pill({
  label,
  strong = false,
  onPress,
}: {
  label: string;
  strong?: boolean;
  onPress: () => void;
}) {
  const styles = useStyles();
  return (
    <Pressable
      accessibilityRole="button"
      onPress={onPress}
      style={[styles.pill, strong && styles.pillStrong]}
    >
      <Text style={[styles.pillLabel, strong && styles.pillLabelStrong]}>{label}</Text>
    </Pressable>
  );
}

const useStyles = themedStyles((colors) => ({
  card: {
    marginHorizontal: 20,
    marginVertical: 8,
    borderRadius: radii.row + 4,
    borderCurve: "continuous",
    borderWidth: 1,
    borderColor: withAlpha(colors.content, 0.08),
    backgroundColor: withAlpha(colors.content, 0.04),
    overflow: "hidden",
  },
  closed: {
    minHeight: 56,
    paddingHorizontal: 14,
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  header: {
    height: 44,
    paddingLeft: 14,
    paddingRight: 6,
    flexDirection: "row",
    alignItems: "center",
    gap: 2,
  },
  title: { flex: 1, fontSize: 13, fontWeight: "500", color: colors.content },
  icon: { width: 36, height: 36, alignItems: "center", justifyContent: "center" },
  web: { flex: 1, backgroundColor: "transparent" },
  loading: { height: ESTIMATE, alignItems: "center", justifyContent: "center" },
  paused: { height: 120, alignItems: "center", justifyContent: "center" },
  footer: {
    minHeight: 48,
    paddingLeft: 14,
    paddingRight: 8,
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  dot: { width: 6, height: 6, borderRadius: 3, backgroundColor: colors.accent },
  status: { flex: 1, fontSize: 12, color: colors.muted },
  pill: {
    height: 32,
    paddingHorizontal: 12,
    borderRadius: 16,
    justifyContent: "center",
    backgroundColor: withAlpha(colors.content, 0.08),
  },
  pillStrong: { backgroundColor: colors.accent },
  pillLabel: { fontSize: 12, fontWeight: "600", color: colors.content },
  pillLabelStrong: { color: "#ffffff" },
}));

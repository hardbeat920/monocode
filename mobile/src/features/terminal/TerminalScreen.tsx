import * as Clipboard from "expo-clipboard";
import { router, Stack } from "expo-router";
import { useHeaderHeight } from "expo-router/react-navigation";
import { useEffect, useMemo, useRef, useState } from "react";
import { Text, View } from "react-native";
import { GestureHandlerRootView } from "react-native-gesture-handler";
import { useKeyboardHandler } from "react-native-keyboard-controller";
import Animated, {
  useAnimatedStyle,
  useSharedValue,
} from "react-native-reanimated";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { scheduleOnRN } from "react-native-worklets";

import { useConnection } from "@/features/connection/ConnectionProvider";
import { CodeHeaderTitle, HeaderButton } from "@/features/files/codeScreen";
import type { Project } from "@/features/workspace/types";
import { CircleButton } from "@/ui/CircleButton";
import { themedStyles, useColors, useScheme } from "@/ui/theme";

import { fromBase64, textBytes, toBase64 } from "./base64";
import { actionBytes, withCtrl } from "./keys";
import { useTerminalActions } from "./terminalActions";
import { canSwipeAway, type Holds } from "./swipeAway";
import { TerminalKeys } from "./TerminalKeys";
import {
  TerminalSession,
  type Resume,
  type TerminalState,
} from "./terminalSession";
import { TerminalSurface, type TerminalHandle } from "./TerminalSurface";

/**
 * Shells the phone left running, by project and worktree, so coming back to a
 * project's terminal finds what was on it.
 */
const shells = new Map<string, Resume>();
const shellKey = (projectId: string, worktreePath?: string) =>
  `${projectId}|${worktreePath ?? ""}`;

const STATUS: Partial<Record<TerminalState, string>> = {
  connecting: "Connecting to your desktop…",
  reconnecting: "Reconnecting…",
};

type Where = { project: Project; worktreePath?: string; run?: string };

/** The project and worktree a terminal is in, under its title. */
export const terminalPlace = (project: Project, worktreePath?: string) =>
  [project.name, worktreePath?.split("/").pop()].filter(Boolean).join(" · ");

/** The terminal as its own modal screen, under the stack's native header. */
export function TerminalScreen({ project, worktreePath, run }: Where) {
  return (
    <TerminalPane
      project={project}
      worktreePath={worktreePath}
      run={run}
      chrome="stack"
      onLeave={() => router.back()}
    />
  );
}

/** The header a sheet draws itself, below its drag indicator. */
const SHEET_HEADER = 68;

/**
 * A shell on the desktop, in a project's folder or one of its worktrees. Leaving
 * keeps it running; ending it is the trash button. `run` is typed and run
 * once the shell is ready.
 *
 * In a `stack` screen the title and buttons go in the native header; in a
 * `sheet` it draws them itself, and the sheet keeps it above the home
 * indicator. Either way the keys ride the keyboard frame by frame, as the
 * session's composer does.
 */
export function TerminalPane({
  project,
  worktreePath,
  run,
  chrome,
  onLeave,
  onSwipeAway,
}: Where & {
  chrome: "stack" | "sheet";
  onLeave: () => void;
  /**
   * Whether a finger is on the header or the keys, where a drag can swipe a
   * sheet away; elsewhere a drag scrolls the terminal.
   */
  onSwipeAway?: (allowed: boolean) => void;
}) {
  const inSheet = chrome === "sheet";
  const headerHeight = useHeaderHeight();
  const top = inSheet ? SHEET_HEADER : headerHeight;
  const { pairing } = useConnection();
  const styles = useStyles();
  const colors = useColors();
  const scheme = useScheme();
  const look = useMemo(
    () => ({
      light: scheme === "light",
      background: colors.background,
      foreground: colors.content,
      cursor: colors.accent,
    }),
    [scheme, colors],
  );
  const safeArea = useSafeAreaInsets();
  const home = inSheet ? 0 : safeArea.bottom;
  const surface = useRef<TerminalHandle>(null);
  const session = useRef<TerminalSession | null>(null);
  const ctrl = useRef(false);
  const [ctrlOn, setCtrlOn] = useState(false);
  const [state, setState] = useState<TerminalState>("connecting");
  const [detail, setDetail] = useState<string>();
  const key = shellKey(project.id, worktreePath);
  const ran = useRef(false);
  const command = useRef(run);
  const [keysHeight, setKeysHeight] = useState(0);
  const [actions] = useTerminalActions(project.id);
  // The keyboard covers the home indicator's space, in a sheet or not.
  const keyboard = useKeyboardRoom(safeArea.bottom);
  const room = Math.max(home, keyboard.room - (safeArea.bottom - home));

  // The shell starts once the view knows how many columns it has.
  const onSize = (size: { cols: number; rows: number }) => {
    if (session.current) return session.current.resize(size.cols, size.rows);
    if (!pairing) return;
    const next = new TerminalSession(
      pairing,
      { projectId: project.id, worktreePath },
      size,
      {
        onOutput: (data, reset) => surface.current?.write(data, reset),
        onState: (value, message) => {
          setState(value);
          setDetail(message);
          if (value === "ended") shells.delete(key);
          if (value === "live" && command.current && !ran.current) {
            ran.current = true;
            next.input(actionBytes(command.current));
          }
        },
      },
      shells.get(key),
    );
    session.current = next;
    void next.start();
  };

  useEffect(
    () => () => {
      const open = session.current;
      session.current = null;
      if (!open) return;
      const resume = open.resume;
      if (resume) shells.set(key, resume);
      open.detach();
    },
    [key],
  );

  const type = (bytes: Uint8Array) => {
    session.current?.input(ctrl.current ? withCtrl(bytes) : bytes);
    if (ctrl.current) {
      ctrl.current = false;
      setCtrlOn(false);
    }
  };

  // Through the page, so a program that asked for bracketed paste gets it.
  const paste = async () => {
    const text = await Clipboard.getStringAsync();
    if (text) surface.current?.paste(toBase64(textBytes(text)));
  };

  const end = async () => {
    const open = session.current;
    session.current = null;
    shells.delete(key);
    onLeave();
    await open?.close();
  };

  const holds = useRef<Holds>({ full: false, chrome: false, pad: false });
  const hold = (next: Partial<Holds>) => {
    holds.current = { ...holds.current, ...next };
    onSwipeAway?.(canSwipeAway(holds.current));
  };
  const chromeTouches = {
    onTouchStart: () => hold({ chrome: true }),
    onTouchEnd: () => hold({ chrome: false }),
    onTouchCancel: () => hold({ chrome: false }),
  };

  return (
    // A sheet is its own native view hierarchy, so its gestures need a root.
    <GestureHandlerRootView style={styles.root}>
      {chrome === "stack" ? (
        <Stack.Screen
          options={{
            // Dragging on the terminal scrolls it in the page, where iOS can't
            // see it: the sheet would take every drag and slide down off the
            // session. Close is the way out.
            gestureEnabled: false,
            headerTitle: () => (
              <CodeHeaderTitle name="Terminal">
                {terminalPlace(project, worktreePath)}
              </CodeHeaderTitle>
            ),
            headerLeft: () => (
              <HeaderButton icon="close" label="Close" onPress={onLeave} />
            ),
            headerRight: () => (
              <HeaderButton
                icon="trash"
                label="End terminal"
                onPress={() => void end()}
              />
            ),
          }}
        />
      ) : (
        <View style={styles.sheetHeader} {...chromeTouches}>
          <CircleButton icon="close" label="Close" onPress={onLeave} />
          <CodeHeaderTitle name="Terminal">
            {terminalPlace(project, worktreePath)}
          </CodeHeaderTitle>
          <CircleButton
            icon="trash"
            label="End terminal"
            onPress={() => void end()}
          />
        </View>
      )}
      {/* Under the keys too: the bar is glass, and history runs on under it. */}
      <View style={[styles.screen, { top, bottom: room }]}>
        <TerminalSurface
          ref={surface}
          look={look}
          onSize={onSize}
          inset={keysHeight}
          onFullScreen={(full) => hold({ full })}
          onReady={(reloaded) => {
            hold({ full: false });
            if (reloaded) session.current?.replay();
          }}
          onInput={(data) => type(fromBase64(data))}
        />
        {STATUS[state] || state === "ended" ? (
          <View style={styles.banner} pointerEvents="none">
            <Text style={styles.bannerText}>
              {state === "ended"
                ? (detail ?? "The terminal ended")
                : STATUS[state]}
            </Text>
          </View>
        ) : null}
      </View>
      <View style={[styles.home, { height: home }]} />
      <Animated.View
        style={[styles.keys, { bottom: home }, keyboard.lift]}
        // The bar's rows fan out over the terminal, and touches between them
        // reach it.
        pointerEvents="box-none"
        {...chromeTouches}
      >
        <TerminalKeys
          ctrl={ctrlOn}
          onCtrl={() => {
            ctrl.current = !ctrl.current;
            setCtrlOn(ctrl.current);
          }}
          onKey={(text) => type(textBytes(text))}
          onPaste={() => void paste()}
          onHideKeyboard={() => surface.current?.blur()}
          actions={actions}
          onAction={(action) => session.current?.input(actionBytes(action))}
          onPadHeld={(pad) => hold({ pad })}
          onHeight={setKeysHeight}
        />
      </Animated.View>
    </GestureHandlerRootView>
  );
}

/**
 * The keyboard, for a screen with a bar on top of it. `lift` moves the bar with
 * the keyboard frame by frame; `room` is the keyboard's height once it is
 * settled (or as it starts to open), so the terminal above resizes once per
 * move rather than every frame, which would make the shell redraw each time.
 * `inset` is the home indicator's space, which the keyboard covers.
 */
function useKeyboardRoom(inset: number) {
  const height = useSharedValue(0);
  const [room, setRoom] = useState(0);
  useKeyboardHandler(
    {
      onStart: (e) => {
        "worklet";
        if (e.height > 0) scheduleOnRN(setRoom, e.height);
      },
      onMove: (e) => {
        "worklet";
        height.set(e.height);
      },
      onInteractive: (e) => {
        "worklet";
        height.set(e.height);
      },
      onEnd: (e) => {
        "worklet";
        height.set(e.height);
        scheduleOnRN(setRoom, e.height);
      },
    },
    [],
  );
  const lift = useAnimatedStyle(() => ({
    transform: [{ translateY: -Math.max(0, height.get() - inset) }],
  }));
  return { room, lift };
}

const useStyles = themedStyles((colors) => ({
  root: { flex: 1, backgroundColor: colors.background },
  screen: { position: "absolute", left: 0, right: 0 },
  keys: { position: "absolute", left: 0, right: 0 },
  sheetHeader: {
    position: "absolute",
    top: 0,
    left: 0,
    right: 0,
    height: SHEET_HEADER,
    paddingTop: 16,
    paddingHorizontal: 16,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
  },
  home: { position: "absolute", left: 0, right: 0, bottom: 0 },
  banner: {
    position: "absolute",
    top: 8,
    alignSelf: "center",
    paddingHorizontal: 12,
    paddingVertical: 6,
    borderRadius: 999,
    backgroundColor: colors.inverseSurface,
  },
  bannerText: { color: colors.inverse, fontSize: 12 },
}));

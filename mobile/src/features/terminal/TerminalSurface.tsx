import {
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  type Ref,
} from "react";
import { WebView, type WebViewMessageEvent } from "react-native-webview";

import {
  BLUR_SCRIPT,
  FOCUS_SCRIPT,
  insetScript,
  lookPreload,
  lookScript,
  pasteScript,
  parsePageMessage,
  run,
  writeScript,
  type TerminalLook,
} from "./terminalBridge";
import { TERMINAL_PAGE } from "./terminalPage.generated";

export type TerminalHandle = {
  /** Draws output; `reset` clears the screen first. */
  write: (data: string, reset: boolean) => void;
  focus: () => void;
  /** Puts the keyboard away. */
  blur: () => void;
  /** Base64 UTF-8 text, pasted as the terminal pastes. */
  paste: (data: string) => void;
};

type Props = {
  look: TerminalLook;
  /** How much of its bottom is under the key bar, where history runs on. */
  inset: number;
  /** Typing, as base64 bytes. */
  onInput: (data: string) => void;
  onSize: (size: { cols: number; rows: number }) => void;
  /** The page is up; `reloaded` when it came back after being lost, empty. */
  onReady?: (reloaded: boolean) => void;
  /** A full-screen program (an editor, a pager) took the screen, or let it go. */
  onFullScreen?: (on: boolean) => void;
  ref?: Ref<TerminalHandle>;
};

/** Output that follows soon after the last draw waits for the next frame. */
const FLUSH_MS = 16;

/**
 * The terminal's screen: xterm.js in a WebView, loaded from a page bundled
 * with the app. The page interprets the shell's output and reports what is
 * typed and how many columns fit. Nothing on it can navigate or reach the
 * network, since it shows a remote shell's output.
 */
export function TerminalSurface({
  look,
  inset,
  onInput,
  onSize,
  onReady,
  onFullScreen,
  ref,
}: Props) {
  const web = useRef<WebView>(null);
  const ready = useRef(false);
  const loads = useRef(0);
  const queue = useRef<string[]>([]);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const drawn = useRef(0);

  const flush = useCallback(() => {
    timer.current = null;
    if (!ready.current || queue.current.length === 0) return;
    const scripts = queue.current;
    queue.current = [];
    drawn.current = Date.now();
    web.current?.injectJavaScript(run(scripts));
  }, []);

  // An echo is drawn at once; a burst after it is gathered a frame at a time.
  const send = useCallback(
    (script: string) => {
      queue.current.push(script);
      if (timer.current) return;
      const wait = drawn.current + FLUSH_MS - Date.now();
      if (wait <= 0) flush();
      else timer.current = setTimeout(flush, wait);
    },
    [flush],
  );

  // The scheme changed while the page was up.
  const shown = useRef(look);
  useEffect(() => {
    if (shown.current === look) return;
    shown.current = look;
    if (ready.current) send(lookScript(look));
  }, [look, send]);

  const covered = useRef(inset);
  useEffect(() => {
    if (covered.current === inset) return;
    covered.current = inset;
    if (ready.current) send(insetScript(inset));
  }, [inset, send]);

  useImperativeHandle(
    ref,
    () => ({
      write: (data, reset) => send(writeScript(data, reset)),
      focus: () => {
        web.current?.requestFocus();
        send(FOCUS_SCRIPT);
      },
      blur: () => send(BLUR_SCRIPT),
      paste: (data) => send(pasteScript(data)),
    }),
    [send],
  );

  const onMessage = (event: WebViewMessageEvent) => {
    const message = parsePageMessage(event.nativeEvent.data);
    if (!message) return;
    if (message.t === "input") onInput(message.data);
    else if (message.t === "size")
      onSize({ cols: message.cols, rows: message.rows });
    else if (message.t === "full") onFullScreen?.(message.on);
    else {
      ready.current = true;
      loads.current += 1;
      onReady?.(loads.current > 1);
      // The scheme may have changed since the page began to load.
      queue.current.unshift(
        lookScript(shown.current),
        insetScript(covered.current),
      );
      flush();
    }
  };

  // The system may end the page's process to free memory; the screen comes
  // back empty, and the app asks the shell for it again.
  const lost = () => {
    ready.current = false;
    queue.current = [];
    web.current?.reload();
  };

  return (
    <WebView
      ref={web}
      style={{ flex: 1, backgroundColor: look.background }}
      source={{ html: TERMINAL_PAGE }}
      injectedJavaScriptBeforeContentLoaded={lookPreload(look)}
      onMessage={onMessage}
      onContentProcessDidTerminate={lost}
      onRenderProcessGone={lost}
      // Only the bundled page: once it has loaded, nothing else may.
      onShouldStartLoadWithRequest={() => !ready.current}
      originWhitelist={["about:*", "data:*"]}
      javaScriptEnabled
      domStorageEnabled={false}
      cacheEnabled={false}
      allowFileAccess={false}
      allowFileAccessFromFileURLs={false}
      allowUniversalAccessFromFileURLs={false}
      allowsLinkPreview={false}
      javaScriptCanOpenWindowsAutomatically={false}
      setSupportMultipleWindows={false}
      mixedContentMode="never"
      // Typing brings up the keyboard without a second tap.
      keyboardDisplayRequiresUserAction={false}
      hideKeyboardAccessoryView
      automaticallyAdjustContentInsets={false}
      contentInsetAdjustmentBehavior="never"
      // WebKit would scroll the page to show the focused input above the
      // keyboard; the screen already makes room for it.
      scrollEnabled={false}
      bounces={false}
      overScrollMode="never"
      textZoom={100}
      accessibilityLabel="Terminal"
    />
  );
}

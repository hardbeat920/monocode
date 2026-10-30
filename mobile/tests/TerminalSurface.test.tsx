import { act, createRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import {
  TerminalSurface,
  type TerminalHandle,
} from "@/features/terminal/TerminalSurface";

type WebProps = {
  onMessage: (event: { nativeEvent: { data: string } }) => void;
  onContentProcessDidTerminate: () => void;
  onShouldStartLoadWithRequest: () => boolean;
};
const web = vi.hoisted(() => ({
  props: null as unknown as WebProps,
  injected: [] as string[],
  reload: vi.fn(),
}));
vi.mock("react-native-webview", async () => {
  const { useImperativeHandle } = await import("react");
  return {
    WebView: (props: WebProps & { ref?: React.Ref<unknown> }) => {
      web.props = props;
      useImperativeHandle(props.ref, () => ({
        injectJavaScript: (script: string) => web.injected.push(script),
        reload: web.reload,
        requestFocus: () => {},
      }));
      return null;
    },
  };
});
vi.mock("@/features/terminal/terminalPage.generated", () => ({
  TERMINAL_PAGE: "<html>",
}));

const look = {
  light: false,
  background: "#000",
  foreground: "#fff",
  cursor: "#09f",
};
const on = { input: vi.fn(), size: vi.fn(), ready: vi.fn(), full: vi.fn() };
const surface = createRef<TerminalHandle>();
let root: ReturnType<typeof createRoot>;
const render = (inset = 0, shown = look) =>
  act(() =>
    root.render(
      <TerminalSurface
        ref={surface}
        look={shown}
        inset={inset}
        onInput={on.input}
        onSize={on.size}
        onReady={on.ready}
        onFullScreen={on.full}
      />,
    ),
  );
const page = (message: object | string) =>
  act(() =>
    web.props.onMessage({
      nativeEvent: {
        data: typeof message === "string" ? message : JSON.stringify(message),
      },
    }),
  );
const scripts = () => web.injected.join("");

beforeEach(() => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  web.injected = [];
  web.reload.mockReset();
  for (const spy of Object.values(on)) spy.mockReset();
  root = createRoot(document.createElement("div"));
  render(90);
});
afterEach(() => {
  act(() => root.unmount());
  vi.useRealTimers();
});

it("holds output until the page is up, then sends its look and inset first", () => {
  act(() => surface.current!.write("aGk=", true));
  expect(web.injected).toEqual([]);
  page({ t: "ready" });
  expect(on.ready).toHaveBeenCalledWith(false);
  const sent = scripts();
  expect(sent.indexOf("__mono.look(")).toBeLessThan(
    sent.indexOf("__mono.inset(90)"),
  );
  expect(sent.indexOf("__mono.inset(90)")).toBeLessThan(
    sent.indexOf('__mono.write("aGk=",true)'),
  );
  // Loaded once, it may not go anywhere else.
  expect(web.props.onShouldStartLoadWithRequest()).toBe(false);
});

it("draws an echo at once, and gathers a burst after it into the next frame", () => {
  page({ t: "ready" });
  // A frame after the page's first draw.
  act(() => vi.advanceTimersByTime(16));
  web.injected = [];
  act(() => surface.current!.write("YQ==", false));
  expect(web.injected).toHaveLength(1);
  act(() => {
    surface.current!.write("Yg==", false);
    surface.current!.write("Yw==", false);
  });
  expect(web.injected).toHaveLength(1);
  act(() => vi.advanceTimersByTime(16));
  expect(web.injected).toHaveLength(2);
  expect(web.injected[1]).toContain("Yg==");
  expect(web.injected[1]).toContain("Yw==");
});

it("tells the page when the key bar covers more or less of it, and the scheme", () => {
  page({ t: "ready" });
  act(() => vi.advanceTimersByTime(16));
  web.injected = [];
  render(130);
  act(() => vi.advanceTimersByTime(16));
  expect(scripts()).toContain("__mono.inset(130)");
  render(130, { ...look, light: true });
  act(() => vi.advanceTimersByTime(16));
  expect(scripts()).toContain('"light":true');
});

it("passes on typing, size and full-screen programs, and drops anything else", () => {
  page({ t: "ready" });
  page({ t: "input", data: "bHM=" });
  page({ t: "size", cols: 80, rows: 24 });
  page({ t: "full", on: true });
  page({ t: "full", on: false });
  page({ t: "open", url: "https://example.com" });
  page("not json");
  expect(on.input).toHaveBeenCalledWith("bHM=");
  expect(on.size).toHaveBeenCalledWith({ cols: 80, rows: 24 });
  expect(on.full.mock.calls).toEqual([[true], [false]]);
});

it("reloads a page the system ended, and says it came back empty", () => {
  page({ t: "ready" });
  act(() => web.props.onContentProcessDidTerminate());
  expect(web.reload).toHaveBeenCalledOnce();
  // Output while it reloads waits for the new page.
  web.injected = [];
  act(() => surface.current!.write("YQ==", false));
  expect(web.injected).toEqual([]);
  page({ t: "ready" });
  expect(on.ready).toHaveBeenLastCalledWith(true);
  expect(scripts()).toContain("__mono.inset(90)");
});

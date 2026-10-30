import { act, forwardRef, isValidElement, useRef, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { Composer } from "@/features/sessions/Composer";

const runtime = vi.hoisted(() => ({
  os: "android",
  scheme: "dark",
  height: 850,
  results: [] as {
    path: string;
    name: string;
    isDir: boolean;
    positions: number[];
    label: string;
    literalPath?: boolean;
  }[],
  values: [] as { get: () => unknown; set: (value: unknown) => void }[],
  photos: vi.fn(async () => []),
  files: vi.fn(async () => []),
}));
function plainText(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(plainText).join("");
  if (
    isValidElement<{ children?: ReactNode; parts?: { text: string }[] }>(node)
  )
    return node.props.parts
      ? node.props.parts.map((part) => part.text).join("")
      : plainText(node.props.children);
  return "";
}
vi.mock("react-native", () => {
  const View = ({
    children,
    style,
  }: {
    children?: ReactNode;
    style?: object;
  }) => <div style={style}>{children}</div>;
  return {
    useWindowDimensions: () => ({ height: runtime.height, width: 400 }),
    Platform: {
      get OS() {
        return runtime.os;
      },
    },
    View,
    ScrollView: ({
      children,
      style,
      contentContainerStyle,
    }: {
      children?: ReactNode;
      style: object;
      contentContainerStyle: object;
    }) => (
      <div data-scroll style={style}>
        <div style={contentContainerStyle}>{children}</div>
      </div>
    ),
    Text: ({ children, style }: { children?: ReactNode; style?: object }) => (
      <span style={style}>{children}</span>
    ),
    Image: () => null,
    Alert: { alert: vi.fn() },
    StyleSheet: {
      absoluteFill: {},
      absoluteFillObject: {},
      create: (styles: object) => styles,
    },
    Modal: ({
      visible,
      children,
    }: {
      visible: boolean;
      children?: ReactNode;
    }) => (visible ? <section>{children}</section> : null),
    Pressable: ({
      children,
      onPress,
      accessibilityLabel,
      disabled,
    }: {
      children?: ReactNode;
      onPress?: () => void;
      accessibilityLabel?: string;
      disabled?: boolean;
    }) => (
      <button
        aria-label={accessibilityLabel}
        onClick={onPress}
        disabled={disabled}
      >
        {children}
      </button>
    ),
    TextInput: forwardRef<
      HTMLTextAreaElement,
      {
        value?: string;
        children?: ReactNode;
        placeholder: string;
        editable: boolean;
        style: object[];
        onChangeText: (text: string) => void;
        onFocus: () => void;
        onBlur: () => void;
        onSelectionChange: (event: {
          nativeEvent: { selection: { start: number; end: number } };
        }) => void;
        onKeyPress: (event: {
          nativeEvent: { key: string };
          preventDefault: () => void;
        }) => void;
      }
    >((props, ref) => (
      <textarea
        ref={ref}
        data-native-rich={props.value === undefined}
        value={props.value ?? plainText(props.children)}
        style={Object.assign({}, ...props.style)}
        placeholder={props.placeholder}
        disabled={!props.editable}
        onFocus={props.onFocus}
        onBlur={props.onBlur}
        onKeyDown={(event) =>
          props.onKeyPress({
            nativeEvent: { key: event.key },
            preventDefault: () => event.preventDefault(),
          })
        }
        onMouseUp={(event) =>
          props.onSelectionChange({
            nativeEvent: {
              selection: {
                start: event.currentTarget.selectionStart,
                end: event.currentTarget.selectionEnd,
              },
            },
          })
        }
        onInput={(event) => props.onChangeText(event.currentTarget.value)}
      />
    )),
  };
});
vi.mock("react-native-reanimated", () => {
  const transition = { duration: () => ({}) };
  return {
    default: {
      View: ({
        children,
        style,
      }: {
        children?: ReactNode;
        style: object | object[];
      }) => {
        const styles = (Array.isArray(style) ? style : [style]).map((item) =>
          typeof item === "function" ? item() : item,
        );
        return <div style={Object.assign({}, ...styles)}>{children}</div>;
      },
    },
    Easing: { out: () => null, cubic: null },
    FadeIn: transition,
    FadeOut: transition,
    interpolate: (
      value: number,
      [a, b]: number[],
      [c, d]: number[],
      clamp?: string,
    ) => {
      const fraction = (value - a) / (b - a);
      return (
        c + (d - c) * (clamp ? Math.min(1, Math.max(0, fraction)) : fraction)
      );
    },
    useAnimatedStyle: (fn: () => object) => fn,
    useSharedValue: (initial: unknown) => {
      const ref = useRef<{
        get: () => unknown;
        set: (value: unknown) => void;
      } | null>(null);
      if (!ref.current) {
        let value = initial;
        ref.current = {
          get: () => value,
          set: (next) => {
            value = next;
          },
        };
        runtime.values.push(ref.current);
      }
      return ref.current;
    },
    withTiming: (value: unknown) => value,
  };
});
vi.mock("@expo/ui/swift-ui", () => ({
  Button: () => null,
  Host: () => null,
  HStack: () => null,
  Menu: () => null,
  Spacer: () => null,
}));
vi.mock("@expo/ui/swift-ui/modifiers", () => ({
  accessibilityLabel: vi.fn(),
  contentShape: vi.fn(),
  frame: vi.fn(),
  shapes: { circle: vi.fn() },
}));
vi.mock("react-native-keyboard-controller", () => ({
  useKeyboardState: (selector: (state: { height: number }) => unknown) =>
    selector({ height: 300 }),
}));
vi.mock("react-native-safe-area-context", () => ({
  useSafeAreaInsets: () => ({ top: 24, bottom: 24 }),
}));
vi.mock("@/features/connection/useMentionSearch", () => ({
  useMentionSearch: () => ({ results: runtime.results, searching: false }),
}));
vi.mock("@/features/connection/ConnectionNotice", () => ({
  useOffline: () => false,
  PulsingDot: () => null,
}));
vi.mock("@/features/files/FileTypeIcon", () => ({ FileTypeIcon: () => null }));
vi.mock("@/features/sessions/SessionScreen", () => ({
  COMPOSER_INPUT_ID: "composer",
}));
vi.mock("@/features/sessions/ProviderLogo", () => ({
  ProviderLogo: () => null,
}));
vi.mock("@/features/sessions/DictationBar", () => ({
  DictationBar: () => null,
}));
vi.mock("@/features/sessions/dictation", () => ({
  canDictate: () => false,
  insertDictation: vi.fn(),
}));
vi.mock("@/features/sessions/attachments", () => ({
  MAX_ATTACHMENTS: 10,
  pickPhotos: runtime.photos,
  pickFiles: runtime.files,
  merge: vi.fn(),
  isImage: () => false,
}));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/ui/LiquidView", () => ({
  LiquidView: ({
    children,
    tintColor,
  }: {
    children?: ReactNode;
    tintColor?: string;
  }) => (
    <div data-liquid-surface data-tint={tintColor}>
      {children}
    </div>
  ),
}));
vi.mock("@/ui/theme", () => ({
  useColors: () => ({
    content: "#ebebeb",
    accent: "#4c9ef5",
    muted: "#8c8c8c",
  }),
  useScheme: () => runtime.scheme,
  themedStyles: (fn: (colors: object) => object) => () =>
    fn({ content: "#ebebeb", accent: "#4c9ef5", muted: "#8c8c8c" }),
}));

let container: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const model = vi.fn();
const send = vi.fn();
const stop = vi.fn(async () => {});
const render = () =>
  act(() =>
    root.render(
      <Composer
        projectId="project"
        context="Project"
        branch="main"
        model="GPT"
        onModelPress={model}
        onSend={send}
        busy
        onStop={stop}
      />,
    ),
  );
const button = (label: string) =>
  container.querySelector<HTMLButtonElement>(`[aria-label="${label}"]`)!;
const modelTouches = () =>
  button("Model: GPT").parentElement!.style.pointerEvents;
beforeEach(() => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  runtime.os = "android";
  runtime.scheme = "dark";
  runtime.height = 850;
  runtime.results = [];
  runtime.values = [];
  vi.clearAllMocks();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

it("excludes the hidden model from Android touches, then permits only its control after expansion", async () => {
  render();
  expect(modelTouches()).toBe("none");
  const input = container.querySelector("textarea")!;
  act(() => input.focus());
  expect(document.activeElement).toBe(input);
  render();
  expect(modelTouches()).toBe("box-none");
  act(() => input.blur());
  render();
  expect(modelTouches()).toBe("none");
  act(() => input.focus());
  act(() => {
    input.value = "Hello";
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(input.value).toBe("Hello");
  expect(model).not.toHaveBeenCalled();
  act(() => input.blur());
  render();
  expect(modelTouches()).toBe("box-none");
  act(() => button("Model: GPT").click());
  expect(model).toHaveBeenCalledOnce();
  await act(async () => button("Send").click());
  expect(send).toHaveBeenCalledWith("Hello", []);
  expect(modelTouches()).toBe("none");
});

it("blocks model hits throughout a morph and dictation, and preserves the stop action", () => {
  render();
  // Shared values: focused, morph progress, dictation progress.
  for (const progress of [0, 0.2, 0.8, 0.999]) {
    runtime.values[1].set(progress);
    render();
    expect(modelTouches()).toBe("none");
  }
  runtime.values[1].set(1);
  runtime.values[2].set(0.2);
  render();
  expect(modelTouches()).toBe("none");
  runtime.values[2].set(0);
  render();
  act(() => button("Stop").click());
  expect(stop).toHaveBeenCalledOnce();
});

it("keeps the iOS hit-testing styles unchanged", () => {
  runtime.os = "ios";
  render();
  expect(modelTouches()).toBe("");
});

it("opens a content-sized attachment menu with safe-area padding and retains both actions", async () => {
  render();
  await act(async () => button("Add photos or files").click());
  const scroll = container.querySelector<HTMLElement>("[data-scroll]")!;
  expect(scroll.style.flexGrow).toBe("0");
  expect(scroll.style.maxHeight).toBe("75%");
  expect((scroll.firstElementChild as HTMLElement).style.paddingBottom).toBe(
    "44px",
  );
  await act(async () =>
    [...container.querySelectorAll("button")]
      .find((item) => item.textContent === "Photo Library")!
      .click(),
  );
  expect(runtime.photos).toHaveBeenCalledWith(10);
  expect(container.querySelector("section")).toBeNull();
  await act(async () => button("Add photos or files").click());
  await act(async () =>
    [...container.querySelectorAll("button")]
      .find((item) => item.textContent === "Files")!
      .click(),
  );
  expect(runtime.files).toHaveBeenCalledWith(10);
});

const type = (input: HTMLTextAreaElement, value: string) =>
  act(() => {
    input.value = value;
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
it.each(["android", "ios"])(
  "selects desktop references and keeps the %s field editable across multiple mentions",
  async (os) => {
    runtime.os = os;
    runtime.results = [
      {
        path: "docs/read me.md",
        name: "read me.md",
        isDir: false,
        positions: [5, 6],
        label: "docs/read-me.md",
      },
    ];
    render();
    const input = container.querySelector("textarea")!;
    act(() => input.focus());
    type(input, "Look at @read");
    expect(container.textContent).toContain("Files and folders");
    expect(container.querySelectorAll("[data-liquid-surface]")).toHaveLength(2);
    act(() => button("docs/read me.md").click());
    expect(input.value).toBe("Look at \u2003read me.md ");
    expect(input.disabled).toBe(false);
    if (os === "ios") expect(input.style.color).toBe("transparent");
    else expect(input.style.color).not.toBe("transparent");
    expect(document.activeElement).toBe(input);
    runtime.results = [
      { path: "src", name: "src", isDir: true, positions: [], label: "src" },
    ];
    type(input, input.value + "and @s");
    act(() => button("src/").click());
    expect(input.value).toBe("Look at \u2003read me.md and \u2003src ");
    await act(async () => button("Send").click());
    expect(send).toHaveBeenCalledWith("Look at @docs/read-me.md and @src", []);
  },
);
it("dismisses suggestions without changing ordinary text and reopens on query edits", () => {
  render();
  const input = container.querySelector("textarea")!;
  type(input, "person@example.com");
  expect(container.textContent).not.toContain("Files and folders");
  type(input, "Try @");
  act(() => button("Dismiss file suggestions").click());
  expect(container.textContent).not.toContain("Files and folders");
  expect(input.value).toBe("Try @");
  type(input, "Try @doc");
  expect(container.textContent).toContain("Files and folders");
});
it("replaces only the mention under the cursor and supports available hardware key events", () => {
  runtime.results = [
    {
      path: "one.ts",
      name: "one.ts",
      isDir: false,
      positions: [],
      label: "one.ts",
    },
    {
      path: "two.ts",
      name: "two.ts",
      isDir: false,
      positions: [],
      label: "two.ts",
    },
  ];
  render();
  const input = container.querySelector("textarea")!;
  type(input, "@first and @partial after");
  act(() => {
    input.setSelectionRange(15, 15);
    input.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
  });
  act(() =>
    input.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
    ),
  );
  act(() =>
    input.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Tab", bubbles: true }),
    ),
  );
  type(input, "@first and @par\ttial after");
  expect(input.value).toBe("@first and \u2003two.ts after");
});

it("bounds suggestions above a tall keyboard on a small screen", () => {
  runtime.height = 550;
  render();
  const input = container.querySelector("textarea")!;
  type(input, "@");
  expect(
    container.querySelector<HTMLElement>("[data-scroll]")!.style.maxHeight,
  ).toBe("44px");
  expect(input.disabled).toBe(false);
});

it.each(["ios", "android"])(
  "sends a legacy desktop spaced-path selection unambiguously on %s",
  async (os) => {
    runtime.os = os;
    runtime.results = [
      {
        path: "docs/read me.md",
        name: "read me.md",
        isDir: false,
        positions: [],
        label: "docs/read%20me.md",
        literalPath: true,
      },
    ];
    render();
    const input = container.querySelector("textarea")!;
    type(input, "Review @read");
    act(() => button("docs/read me.md").click());
    expect(input.value).toBe("Review \u2003read me.md ");
    await act(async () => button("Send").click());
    expect(send).toHaveBeenCalledWith("Review `docs/read me.md`", []);
  },
);

it("reopens suggestions when a focused cursor enters a mention after refocusing outside it", () => {
  render();
  const input = container.querySelector("textarea")!;
  act(() => input.focus());
  type(input, "Look @src after");
  act(() => input.blur());
  act(() => input.focus());
  expect(container.textContent).not.toContain("Files and folders");
  act(() => {
    input.setSelectionRange(8, 8);
    input.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
  });
  expect(container.textContent).toContain("Files and folders");
});

it.each(["ios", "android"])(
  "shows duplicate nested basenames with distinct canonical references on %s",
  async (os) => {
    runtime.os = os;
    runtime.results = [
      {
        path: "web/src/main.tsx",
        name: "main.tsx",
        label: "web/src/main.tsx",
        isDir: false,
        positions: [],
      },
    ];
    render();
    const input = container.querySelector("textarea")!;
    act(() => input.focus());
    type(input, "Compare @main");
    act(() => button("web/src/main.tsx").click());
    expect(input.value).toBe("Compare \u2003main.tsx ");
    runtime.results = [
      {
        path: "host/main.tsx",
        name: "main.tsx",
        label: "host/main.tsx",
        isDir: false,
        positions: [],
      },
    ];
    type(input, input.value + "and @main");
    act(() => button("host/main.tsx").click());
    expect(input.value).toBe("Compare \u2003main.tsx and \u2003main.tsx ");
    expect(input.disabled).toBe(false);
    await act(async () => button("Send").click());
    expect(send).toHaveBeenCalledWith(
      "Compare @web/src/main.tsx and @host/main.tsx",
      [],
    );
  },
);

it("uses Android native styled text with one text source instead of a transparent editor", () => {
  render();
  const input = container.querySelector("textarea")!;
  expect(input.dataset.nativeRich).toBe("true");
  type(input, "ordinary text");
  expect(input.value).toBe("ordinary text");
  expect(input.style.color).not.toBe("transparent");
  expect(input.style.backgroundColor).toBe("transparent");
  expect(input.style.color).toBe("#ebebeb");
});

it("uses a white blur tint on both Android composer and picker in light mode", () => {
  runtime.scheme = "light";
  render();
  type(container.querySelector("textarea")!, "@");
  expect(
    [...container.querySelectorAll("[data-liquid-surface]")].map((surface) =>
      surface.getAttribute("data-tint"),
    ),
  ).toEqual(["rgba(255,255,255,0.88)", "rgba(255,255,255,0.88)"]);
});

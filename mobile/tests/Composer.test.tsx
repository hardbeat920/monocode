import {
  act,
  forwardRef,
  useRef,
  type ComponentProps,
  type ReactNode,
} from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { Alert } from "react-native";
import { Composer } from "@/features/sessions/Composer";

const runtime = vi.hoisted(() => ({
  os: "android",
  offline: false,
  values: [] as { get: () => unknown; set: (value: unknown) => void }[],
  photos: vi.fn(async () => []),
  files: vi.fn(async () => []),
  paste: vi.fn(async () => [] as object[]),
  clipboard: null as ((hasImage: boolean) => void) | null,
}));
vi.mock("react-native", () => {
  const View = ({
    children,
    style,
  }: {
    children?: ReactNode;
    style?: object;
  }) => <div style={style}>{children}</div>;
  return {
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
    Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
    Image: () => null,
    Alert: { alert: vi.fn() },
    StyleSheet: { absoluteFill: {} },
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
        value: string;
        placeholder: string;
        editable: boolean;
        onChangeText: (text: string) => void;
        onFocus: () => void;
        onBlur: () => void;
      }
    >((props, ref) => (
      <textarea
        ref={ref}
        value={props.value}
        placeholder={props.placeholder}
        disabled={!props.editable}
        onFocus={props.onFocus}
        onBlur={props.onBlur}
        onInput={(event) => props.onChangeText(event.currentTarget.value)}
      />
    )),
  };
});
vi.mock("@/features/sessions/TypingPostureFocus", () => ({
  TypingPostureFocus: ({ enabled, onEnterTypingPosture }: {
    enabled: boolean; onEnterTypingPosture: () => void;
  }) => <button aria-label="Enter typing posture" disabled={!enabled} onClick={onEnterTypingPosture} />,
}));
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
vi.mock("expo-router", () => ({
  useNavigation: () => ({ addListener: () => () => {} }),
}));
vi.mock("react-native-safe-area-context", () => ({
  useSafeAreaInsets: () => ({ bottom: 24 }),
}));
vi.mock("@/features/connection/ConnectionNotice", () => ({
  useOffline: () => runtime.offline,
  PulsingDot: () => <i data-testid="pulsing-dot" />,
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
// The real merge, restore and limits; only what reaches native code is stood in for.
vi.mock("expo-clipboard", () => ({}));
vi.mock("expo-file-system", () => ({}));
vi.mock("expo-image-manipulator", () => ({}));
vi.mock("expo-image-picker", () => ({}));
vi.mock("@/features/sessions/attachments", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/features/sessions/attachments")>();
  return {
    ...actual,
    pickPhotos: runtime.photos,
    pickFiles: runtime.files,
    pasteImage: runtime.paste,
    watchClipboardImage: (onChange: (hasImage: boolean) => void) => {
      runtime.clipboard = onChange;
      return () => {
        runtime.clipboard = null;
      };
    },
  };
});
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/ui/LiquidView", () => ({ LiquidView: () => null }));
vi.mock("@/ui/theme", () => ({
  useColors: () => ({}),
  useScheme: () => "dark",
  themedStyles: (fn: (colors: object) => object) => () => fn({}),
}));

let container: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const model = vi.fn();
const send = vi.fn();
const stop = vi.fn(async () => {});
const render = (
  active = true,
  props: Partial<ComponentProps<typeof Composer>> = {},
) =>
  act(() =>
    root.render(
      <Composer
        active={active}
        context="Project"
        branch="main"
        model="GPT"
        onModelPress={model}
        onSend={send}
        busy
        onStop={stop}
        {...props}
      />,
    ),
  );
const button = (label: string) =>
  container.querySelector<HTMLButtonElement>(`[aria-label="${label}"]`)!;
const type = (text: string) => {
  const input = container.querySelector("textarea")!;
  act(() => {
    input.value = text;
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  return input;
};
const modelTouches = () =>
  button("Model: GPT").parentElement!.style.pointerEvents;
beforeEach(() => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  runtime.os = "android";
  runtime.offline = false;
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
  expect(scroll.parentElement!.style.maxHeight).toBe("80%");
  expect((scroll.firstElementChild as HTMLElement).style.paddingBottom).toBe(
    "40px",
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

// The native observer handles posture transitions; the composer focuses its
// existing input, preserving its draft and leaving inactive routes alone.
it("focuses the existing draft on typing posture only for the active route", () => {
  render();
  const input = container.querySelector("textarea")!;
  act(() => {
    input.value = "Unsent draft";
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  act(() => button("Enter typing posture").click());
  expect(document.activeElement).toBe(input);
  expect(input.value).toBe("Unsent draft");
  act(() => input.blur());
  render(false);
  act(() => button("Enter typing posture").click());
  expect(document.activeElement).not.toBe(input);
  expect(input.value).toBe("Unsent draft");
});

it("keeps the field open while the desktop is away and holds the message until it is back", async () => {
  runtime.offline = true;
  render();
  const input = container.querySelector("textarea")!;
  expect(input.disabled).toBe(false);
  expect(input.placeholder).toBe("Reconnecting to desktop…");
  expect(container.querySelector('[data-testid="pulsing-dot"]')).toBeNull();

  type("Written on the train");
  expect(input.value).toBe("Written on the train");
  expect(button("Send").disabled).toBe(true);
  expect(container.querySelector('[data-testid="pulsing-dot"]')).not.toBeNull();
  act(() => button("Send").click());
  expect(send).not.toHaveBeenCalled();
  expect(input.value).toBe("Written on the train");

  runtime.offline = false;
  render();
  expect(button("Send").disabled).toBe(false);
  expect(container.querySelector('[data-testid="pulsing-dot"]')).toBeNull();
  await act(async () => button("Send").click());
  expect(send).toHaveBeenCalledWith("Written on the train", []);
});

it("can't stop a turn while the desktop is away, but dictation still works", () => {
  runtime.offline = true;
  render();
  expect(button("Stop").disabled).toBe(true);
  act(() => button("Stop").click());
  expect(stop).not.toHaveBeenCalled();

  render(true, { busy: false });
  expect(button("Dictate").disabled).toBe(false);
});

it("leaves a session's error for when the desktop is back, so it can't lock the field", () => {
  runtime.offline = true;
  const notice = { text: "Turn failed", dismiss: vi.fn() };
  render(true, { notice });
  expect(container.querySelector("textarea")!.disabled).toBe(false);
  expect(container.textContent).not.toContain("Turn failed");

  runtime.offline = false;
  render(true, { notice });
  expect(container.querySelector("textarea")!.disabled).toBe(true);
  expect(container.textContent).toContain("Turn failed");
});

it("offers to paste a copied image and holds it as a picked photo", async () => {
  const pasted = { id: "p", name: "Pasted image.png", mimeType: "image/png", size: 3, uri: "file:///p.png", data: "AAA" };
  runtime.paste.mockResolvedValueOnce([pasted]);
  render();
  const menuItems = () => [...container.querySelectorAll("button")].map((item) => item.textContent);
  await act(async () => button("Add photos or files").click());
  expect(menuItems()).not.toContain("Paste Image");
  await act(async () => button("Add photos or files").click());

  act(() => runtime.clipboard!(true));
  await act(async () => button("Add photos or files").click());
  expect(menuItems()).toContain("Paste Image");
  await act(async () =>
    [...container.querySelectorAll("button")].find((item) => item.textContent === "Paste Image")!.click(),
  );
  expect(runtime.paste).toHaveBeenCalledWith(10);
  // The same strip as a picked photo, and sent with the message.
  expect(button("Remove Pasted image.png")).toBeTruthy();
  await act(async () => button("Send").click());
  expect(send).toHaveBeenCalledWith("", [pasted]);

  act(() => runtime.clipboard!(false));
  await act(async () => button("Add photos or files").click());
  expect(menuItems()).not.toContain("Paste Image");
});

it("keeps every image when a second pick lands while the first paste is pending", async () => {
  let releaseFirst!: (picked: object[]) => void;
  runtime.paste
    .mockReturnValueOnce(new Promise<object[]>((resolve) => (releaseFirst = resolve)))
    .mockResolvedValueOnce([chip("B.png")]);
  runtime.photos.mockResolvedValueOnce([chip("C.jpg")] as never);
  render();
  act(() => runtime.clipboard!(true));
  // The first waits, as on iOS's "Allow Paste" prompt; a second paste and a photo land meanwhile.
  await choose("Paste Image");
  await choose("Paste Image");
  await choose("Photo Library");
  await act(async () => releaseFirst([chip("A.png")]));
  expect(button("Remove A.png")).toBeTruthy();
  expect(button("Remove B.png")).toBeTruthy();
  expect(button("Remove C.jpg")).toBeTruthy();
});

const chip = (name: string, data = "AAA") => ({
  id: name,
  name,
  mimeType: "image/png",
  size: 3,
  uri: `file:///${name}`,
  data,
});
const choose = async (label: string) => {
  await act(async () => button("Add photos or files").click());
  await act(async () =>
    [...container.querySelectorAll("button")].find((item) => item.textContent === label)!.click(),
  );
};
const held = () =>
  [...container.querySelectorAll("button")]
    .map((item) => item.getAttribute("aria-label") ?? "")
    .filter((label) => label.startsWith("Remove "))
    .map((label) => label.slice("Remove ".length));
/** A send the desktop hasn't answered yet; `fail` is its answer. */
const pendingSend = () => {
  let fail!: (reason: Error) => void;
  send.mockReturnValueOnce(new Promise<void>((_, reject) => (fail = reject)));
  return (reason: Error) => fail(reason);
};

it("removes a pasted image from the strip", async () => {
  runtime.paste.mockResolvedValueOnce([chip("A.png")]);
  render();
  act(() => runtime.clipboard!(true));
  await choose("Paste Image");
  expect(held()).toEqual(["A.png"]);
  act(() => button("Remove A.png").click());
  expect(held()).toEqual([]);
});

it("keeps a failed send's images alongside one pasted while it was sending", async () => {
  runtime.paste.mockResolvedValueOnce([chip("A.png")]).mockResolvedValueOnce([chip("B.png")]);
  render(true, { busy: false });
  act(() => runtime.clipboard!(true));
  await choose("Paste Image");
  type("Look at this");
  const fail = pendingSend();
  await act(async () => button("Send").click());
  expect(send).toHaveBeenCalledWith("Look at this", [chip("A.png")]);
  expect(held()).toEqual([]);
  await choose("Paste Image");
  expect(held()).toEqual(["B.png"]);
  await act(async () => fail(new Error("Desktop went away")));
  // The text's image first, then the new one, each once.
  expect(held()).toEqual(["A.png", "B.png"]);
  expect(container.querySelector("textarea")!.value).toBe("Look at this");
  expect(Alert.alert).toHaveBeenCalledWith("Message not sent", "Desktop went away");
  // Both go with the retry.
  send.mockResolvedValueOnce(undefined);
  await act(async () => button("Send").click());
  expect(send).toHaveBeenLastCalledWith("Look at this", [chip("A.png"), chip("B.png")]);
});

it("keeps a failed send's full set and says which image pasted meanwhile didn't fit", async () => {
  const ten = Array.from({ length: 10 }, (_, i) => chip(`${i}.png`));
  runtime.photos.mockResolvedValueOnce(ten as never);
  runtime.paste.mockResolvedValueOnce([chip("Late.png")]);
  render(true, { busy: false });
  await choose("Photo Library");
  expect(held()).toHaveLength(10);
  const fail = pendingSend();
  await act(async () => button("Send").click());
  act(() => runtime.clipboard!(true));
  await choose("Paste Image");
  await act(async () => fail(new Error("Desktop went away")));
  expect(held()).toEqual(ten.map((item) => item.name));
  expect(Alert.alert).toHaveBeenCalledWith(
    "Message not sent",
    expect.stringMatching(/^Desktop went away\n\nA message can carry up to 10 files.*Late\.png.*was removed\.$/s),
  );
});

it("drops an image pasted meanwhile that would take the restored message past the size limit", async () => {
  const big = (name: string) => chip(name, "A".repeat(10 * 1024 * 1024));
  runtime.photos.mockResolvedValueOnce([big("One.png"), big("Two.png")] as never);
  runtime.paste.mockResolvedValueOnce([big("Three.png")]);
  render(true, { busy: false });
  await choose("Photo Library");
  const fail = pendingSend();
  await act(async () => button("Send").click());
  act(() => runtime.clipboard!(true));
  await choose("Paste Image");
  expect(held()).toEqual(["Three.png"]);
  await act(async () => fail(new Error("Desktop went away")));
  expect(held()).toEqual(["One.png", "Two.png"]);
  expect(Alert.alert).toHaveBeenCalledWith("Message not sent", expect.stringContaining("Three.png"));
});

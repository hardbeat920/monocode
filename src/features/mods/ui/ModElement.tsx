/**
 * Renders a mod's element tree (types.ts `Element`) with Monocode's own
 * components. Trees are untrusted data from the mod worker: unknown types
 * render nothing, tones are a closed set, and callbacks arrive as
 * `{ $cb: id }` that go back to the host on press.
 */
import {
  createContext,
  useContext,
  useEffect,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import type {
  BoxProps,
  ButtonProps,
  InputProps,
  Node,
  TextProps,
  Tone,
  TreeProps,
  WireCallback,
} from "../api/types";
import { iconByName } from "../../../shared/ui/icons";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import { IconButton } from "../../../app/shell/TitleBar";
import { Badge } from "../../../shared/ui/Badge";
import { Skeleton } from "../../../shared/ui/Skeleton";
import { Shimmer } from "../../../shared/ui/Shimmer";
import { TerminalSpinner } from "../../sessions/ui/TerminalSpinner";
import { AgentMarkdown } from "../../sessions/ui/AgentMarkdown";
import { ModTree } from "./ModTree";

const TONE_COLOR: Record<Tone, string> = {
  content: "var(--color-content)",
  muted: "color-mix(in srgb, var(--color-content) 55%, transparent)",
  subtle: "color-mix(in srgb, var(--color-content) 35%, transparent)",
  accent: "var(--color-accent)",
  success: "var(--color-diff-add-fg)",
  danger: "var(--color-diff-del-fg)",
  warning: "var(--tone-warning)",
  info: "var(--tone-info)",
  read: "var(--tone-read)",
  write: "var(--tone-write)",
  git: "var(--tone-git)",
  purple: "var(--tone-purple)",
  orange: "var(--tone-orange)",
  green: "var(--tone-green)",
  teal: "var(--tone-teal)",
  pink: "var(--tone-pink)",
  cyan: "var(--tone-cyan)",
  yellow: "var(--tone-yellow)",
  blue: "var(--tone-blue)",
  red: "var(--tone-red)",
};

/** CSS colour for a tone; anything outside the closed set is ignored. */
export function toneColor(tone: unknown): string | undefined {
  return typeof tone === "string" &&
    Object.prototype.hasOwnProperty.call(TONE_COLOR, tone)
    ? TONE_COLOR[tone as Tone]
    : undefined;
}

/** A soft fill of a tone, for badges and backgrounds. */
export function toneTint(tone: unknown, percent = 14): string | undefined {
  const color = toneColor(tone);
  return color && `color-mix(in srgb, ${color} ${percent}%, transparent)`;
}

export function callbackId(value: unknown): string | undefined {
  const id = (value as WireCallback | null | undefined)?.$cb;
  return typeof value === "object" && typeof id === "string" ? id : undefined;
}

type ModRender = {
  /** Sends a press back to the mod; ignores props that are not callbacks. */
  invoke: (callback: unknown, args?: unknown[]) => void;
  /** Project root for `Tree` rows with a `path` (git colours). */
  cwd?: string;
};

const ModRenderContext = createContext<ModRender>({ invoke: () => {} });

export function useModRender(): ModRender {
  return useContext(ModRenderContext);
}

export function ModElement({
  node,
  onInvoke,
  cwd,
}: {
  node: Node;
  onInvoke: (callback: string, args: unknown[]) => void;
  cwd?: string;
}) {
  const invoke = (callback: unknown, args: unknown[] = []) => {
    const id = callbackId(callback);
    if (id) onInvoke(id, args);
  };
  return (
    <ModRenderContext.Provider value={{ invoke, cwd }}>
      <ModNode node={node} />
    </ModRenderContext.Provider>
  );
}

function ModNodes({ nodes }: { nodes: unknown }) {
  if (!Array.isArray(nodes)) return null;
  return (
    <>
      {nodes.map((child, index) => (
        <ModNode key={childKey(child, index)} node={child as Node} />
      ))}
    </>
  );
}

function childKey(node: unknown, index: number): string {
  const key = (node as { props?: { key?: unknown } } | null)?.props?.key;
  return typeof key === "string" || typeof key === "number"
    ? `k:${key}`
    : `i:${index}`;
}

function ModNode({ node }: { node: Node }) {
  if (typeof node === "string" || typeof node === "number") return <>{node}</>;
  if (!node || typeof node !== "object") return null;
  const props = (node.props ?? {}) as Record<string, unknown>;
  switch (node.type) {
    case "Box":
      return <ModBox props={props as BoxProps} nodes={node.children} />;
    case "Text":
      return <ModText props={props as TextProps} nodes={node.children} />;
    case "Button":
      return <ModButton props={props as ButtonProps} />;
    case "Input":
      return <ModInput props={props as InputProps} />;
    case "Badge":
      return (
        <Badge
          style={{
            color: toneColor(props.tone) ?? TONE_COLOR.muted,
            backgroundColor: toneTint(props.tone ?? "content", 12),
          }}
        >
          {String(props.label ?? "")}
        </Badge>
      );
    case "Dot":
      return (
        <span
          aria-hidden
          className="inline-block size-1.5 shrink-0 rounded-full"
          style={{ backgroundColor: toneColor(props.tone) ?? TONE_COLOR.muted }}
        />
      );
    case "Icon": {
      const Icon = iconByName(String(props.name));
      return Icon ? (
        <Icon
          aria-hidden
          className="size-3.5 shrink-0"
          style={{ color: toneColor(props.tone) }}
        />
      ) : null;
    }
    case "Spinner":
      return <TerminalSpinner />;
    case "Skeleton":
      return <Skeleton rows={Number(props.rows) || 1} />;
    case "Markdown":
      // AgentMarkdown sanitizes raw HTML and blocks remote media.
      return <AgentMarkdown text={String(props.text ?? "")} />;
    case "Tree":
      return <ModTree props={props as TreeProps} />;
    default:
      return null;
  }
}

const SPACE = new Set([0, 1, 2, 3, 4, 6, 8]);

function space(value: unknown): number | undefined {
  return typeof value === "number" && SPACE.has(value) ? value * 4 : undefined;
}

const ALIGN = { start: "flex-start", center: "center", end: "flex-end", stretch: "stretch" } as const;
const JUSTIFY = { start: "flex-start", center: "center", end: "flex-end", between: "space-between" } as const;

function pick<T extends Record<string, string>>(map: T, key: unknown): T[keyof T] | undefined {
  return typeof key === "string" && Object.prototype.hasOwnProperty.call(map, key)
    ? map[key as keyof T]
    : undefined;
}

/** React writes "" for undefined keys, which would clear a `padding` shorthand. */
function defined(style: Record<string, unknown>): CSSProperties {
  return Object.fromEntries(
    Object.entries(style).filter(([, value]) => value !== undefined),
  ) as CSSProperties;
}

function ModBox({ props, nodes }: { props: BoxProps; nodes: unknown }) {
  const { invoke } = useModRender();
  const pressable = callbackId(props.onPress) != null;
  const style = defined({
    flexDirection: props.flexDirection === "column" ? "column" : "row",
    flexGrow: typeof props.flexGrow === "number" ? props.flexGrow : undefined,
    flexShrink: typeof props.flexShrink === "number" ? props.flexShrink : undefined,
    flexWrap: props.flexWrap === "wrap" ? "wrap" : undefined,
    alignItems: pick(ALIGN, props.alignItems),
    justifyContent: pick(JUSTIFY, props.justifyContent),
    gap: space(props.gap),
    padding: space(props.padding),
    paddingLeft: space(props.paddingX),
    paddingRight: space(props.paddingX),
    paddingTop: space(props.paddingY),
    paddingBottom: space(props.paddingY),
    marginTop: space(props.marginTop),
    borderColor: toneColor(props.borderColor),
    backgroundColor: toneTint(props.backgroundColor),
    "--mod-hover-border": toneColor(props.hoverBorderColor),
  });
  const className = `flex min-w-0 ${
    props.border ? "rounded-md border border-stroke" : ""
  } ${
    props.hoverBorderColor ? "hover:[border-color:var(--mod-hover-border)]" : ""
  } ${props.scroll ? "min-h-0 overflow-auto" : ""} ${
    pressable
      ? "cursor-default rounded-md hover:bg-content/5 focus-visible:outline-1 focus-visible:outline-accent/40"
      : ""
  }`;
  if (!pressable) {
    return (
      <div style={style} className={className}>
        <ModNodes nodes={nodes} />
      </div>
    );
  }
  return (
    <div
      role="button"
      tabIndex={0}
      style={style}
      className={className}
      onClick={(event) => {
        // A control inside the box handles its own press.
        const hit = (event.target as Element).closest(
          "button, input, [role='button']",
        );
        if (hit && hit !== event.currentTarget) return;
        invoke(props.onPress);
      }}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return;
        if (event.key !== "Enter" && event.key !== " ") return;
        event.preventDefault();
        invoke(props.onPress);
      }}
    >
      <ModNodes nodes={nodes} />
    </div>
  );
}

function ModText({ props, nodes }: { props: TextProps; nodes: unknown }) {
  const shimmer = toneColor(props.shimmer);
  const color = shimmer ?? toneColor(props.color);
  const truncate = props.wrap === "truncate-end" || props.wrap === "truncate-start";
  const className = `min-w-0 ${props.bold ? "font-semibold" : ""} ${
    props.italic ? "italic" : ""
  } ${props.dimColor ? "opacity-60" : ""} ${
    props.strikethrough ? "line-through" : ""
  } ${props.mono ? "font-mono" : ""} ${
    truncate ? "truncate" : "whitespace-pre-wrap break-words"
  }`;
  const style: CSSProperties = {
    color,
    // Ellipsis on the left: the start of the string is what gets cut.
    ...(props.wrap === "truncate-start"
      ? { direction: "rtl", textAlign: "left" }
      : {}),
  };
  const only = Array.isArray(nodes) && nodes.length === 1 ? nodes[0] : null;
  const body =
    shimmer && (typeof only === "string" || typeof only === "number") ? (
      <Shimmer>{String(only)}</Shimmer>
    ) : (
      <ModNodes nodes={nodes} />
    );
  return (
    <span style={style} className={className}>
      {props.wrap === "truncate-start" ? <bdi>{body}</bdi> : body}
    </span>
  );
}

function ModButton({ props }: { props: ButtonProps }) {
  const { invoke } = useModRender();
  const Icon = iconByName(props.icon);
  const label = typeof props.label === "string" ? props.label : "";
  const title = typeof props.title === "string" ? props.title : undefined;
  const press = () => {
    if (!props.disabled) invoke(props.onPress);
  };
  if (!label && Icon) {
    return (
      <IconButton
        label={title ?? props.icon ?? ""}
        disabled={!!props.disabled}
        onClick={press}
      >
        <Icon className="size-3.5" strokeWidth={1.75} />
      </IconButton>
    );
  }
  if (props.plain) {
    return (
      <button
        type="button"
        title={title}
        disabled={!!props.disabled}
        onClick={press}
        className={`inline-flex shrink-0 items-center gap-1 rounded px-1 hover:bg-content/10 disabled:opacity-40 ${
          props.tone === "danger"
            ? "text-red-400"
            : "text-content/55 hover:text-content"
        }`}
      >
        {Icon ? <Icon className="size-3.5" strokeWidth={1.75} /> : null}
        {label}
      </button>
    );
  }
  return (
    <SecondaryButton
      title={title}
      danger={props.tone === "danger"}
      disabled={!!props.disabled}
      onClick={press}
    >
      {Icon ? <Icon className="size-3.5" strokeWidth={1.75} /> : null}
      {label}
    </SecondaryButton>
  );
}

function ModInput({ props }: { props: InputProps }) {
  const { invoke } = useModRender();
  const external = typeof props.value === "string" ? props.value : "";
  const [value, setValue] = useState(external);
  // The mod owns the value; adopt it whenever it sends a new one.
  useEffect(() => setValue(external), [external]);
  const onKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Enter" || event.nativeEvent.isComposing) return;
    event.preventDefault();
    invoke(props.onSubmit, [value]);
    // An echo of the same value skips the effect, so adopt it here to let submit-then-clear work.
    setValue(external);
  };
  return (
    <label className="flex h-7 min-w-0 flex-1 items-center gap-2 rounded-md border border-content/10 px-2 focus-within:border-content/20">
      {typeof props.label === "string" && props.label ? (
        <span className="shrink-0 text-content/55">{props.label}</span>
      ) : null}
      <input
        value={value}
        placeholder={
          typeof props.placeholder === "string" ? props.placeholder : undefined
        }
        autoFocus={!!props.autoFocus}
        spellCheck={false}
        onChange={(event) => {
          setValue(event.target.value);
          invoke(props.onInput, [event.target.value]);
        }}
        onKeyDown={onKeyDown}
        className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/35"
      />
    </label>
  );
}

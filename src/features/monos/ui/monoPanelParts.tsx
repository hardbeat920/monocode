import {
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type TextareaHTMLAttributes,
} from "react";
import { ChevronLeft } from "../../../shared/ui/icons";
import { IconButton } from "../../../app/shell/TitleBar";
import { PROJECT_MASCOTS } from "../../projects/model/projectMascots";
import { MONO_COLORS } from "../model/mono";
import {
  ColorPickerPopover,
  ColorSwatchRow,
} from "../../../shared/ui/ColorPickerPopover";
import { normalizeHex } from "../../../shared/lib/colorUtils";
import { Popover } from "../../../shared/ui/Popover";
import { PixelMascot } from "../../projects/ui/PixelMascot";
import { MarkdownSourceEditor } from "../../sessions/ui/MarkdownSourceEditor";
import {
  MEMORY_MAX_BYTES,
  MEMORY_MAX_LINES,
  memoryWithinBudget,
  MonoFileConflict,
  saveMonoFile,
  type MonoFile,
} from "../model/monoFiles";

/** The top of a page in the Mono's panel: back, a title, and its actions. */
export function PageHeader({
  title,
  onBack,
  children,
}: {
  title: ReactNode;
  onBack: () => void;
  children?: ReactNode;
}) {
  return (
    // Same height as the panel's own header, so a page does not jump.
    <header
      className="flex h-10 shrink-0 items-center gap-1 border-b border-stroke pl-2"
      style={{
        paddingRight: "calc(0.75rem + var(--mono-window-controls-width, 0px))",
      }}
      data-tauri-drag-region="deep"
    >
      <IconButton label="Back" onClick={onBack}>
        <ChevronLeft className="size-3.5" strokeWidth={1.75} />
      </IconButton>
      <h3 className="min-w-0 flex-1 truncate text-[13px] font-medium text-content">
        {title}
      </h3>
      {children}
    </header>
  );
}

export function Section({
  title,
  action,
  children,
}: {
  title: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="border-t border-stroke px-2 pb-4 pt-3">
      <div className="flex items-center justify-between px-2 pb-2">
        <h4 className="text-[11px] font-medium uppercase tracking-wide text-content/40">
          {title}
        </h4>
        {action}
      </div>
      {children}
    </section>
  );
}

export function Property({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div className="grid grid-cols-[5.5rem_minmax(0,1fr)] items-start text-[12px]">
      <dt className="flex h-7 items-center text-content/45">{label}</dt>
      <dd className="flex min-h-7 min-w-0 items-center text-content/85">
        {children}
      </dd>
    </div>
  );
}

export function Hint({ children }: { children: ReactNode }) {
  return (
    <p className="mt-2 px-2 text-[11px] leading-4 text-content/35">
      {children}
    </p>
  );
}

/** A textarea that grows with what is typed, scrolling past `maxHeight`. */
export function AutoTextarea({
  maxHeight = 320,
  ...props
}: TextareaHTMLAttributes<HTMLTextAreaElement> & {
  value: string;
  maxHeight?: number;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, maxHeight)}px`;
    el.style.overflowY = el.scrollHeight > maxHeight ? "auto" : "hidden";
  }, [props.value, maxHeight]);
  return <textarea ref={ref} {...props} />;
}

export function Empty({ children }: { children: ReactNode }) {
  return (
    <p className="px-2 py-6 text-center text-[12px] text-content/45">
      {children}
    </p>
  );
}

export function MascotPicker({
  current,
  color,
  onPick,
}: {
  current: string;
  color: string;
  onPick: (mascot: string) => void;
}) {
  return (
    <div
      role="radiogroup"
      aria-label="Mascot"
      className="grid grid-cols-10 gap-0.5 px-1"
    >
      {PROJECT_MASCOTS.map(({ name }) => {
        const selected = name === current;
        return (
          <button
            key={name}
            type="button"
            role="radio"
            aria-checked={selected}
            aria-label={name}
            title={name.charAt(0).toUpperCase() + name.slice(1)}
            onClick={() => onPick(name)}
            className={`grid h-8 min-w-0 place-items-center rounded-md ${
              selected
                ? "bg-selection ring-1 ring-content/20"
                : "hover:bg-content/6"
            }`}
          >
            <PixelMascot
              name={name}
              color={color}
              still={!selected}
              className="size-5"
            />
          </button>
        );
      })}
    </div>
  );
}

/** Nine presets and the project's custom picker, aligned with the mascots. */
export function ColorPicker({
  current,
  onPick,
  inline = false,
}: {
  current: string;
  onPick: (color: string) => void;
  /** Opens the custom picker in place, for use inside another popover. */
  inline?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLDivElement>(null);
  const presetIndex = MONO_COLORS.findIndex((color) => color === current);
  return (
    <div ref={anchor} role="group" aria-label="Color">
      <ColorSwatchRow
        className="grid grid-cols-10 place-items-center gap-0.5 px-1"
        colors={MONO_COLORS}
        labels={[
          "Blue",
          "Coral",
          "Yellow",
          "Green",
          "Pink",
          "Purple",
          "Teal",
          "Orange",
          "Indigo",
        ]}
        colorIndex={presetIndex >= 0 ? presetIndex : undefined}
        customColor={presetIndex < 0 ? current : undefined}
        customPickerOpen={open}
        onPickIndex={(index) => {
          setOpen(false);
          onPick(MONO_COLORS[index]);
        }}
        onToggleCustom={() => setOpen((value) => !value)}
      />
      {open && inline ? (
        <div className="px-1 pt-2">
          <ColorPickerPopover value={normalizeHex(current)} onChange={onPick} />
        </div>
      ) : open ? (
        <Popover
          anchor={anchor}
          side="bottom"
          align="end"
          width={248}
          onDismiss={() => setOpen(false)}
          className="px-2 pb-2"
        >
          <ColorPickerPopover value={normalizeHex(current)} onChange={onPick} />
        </Popover>
      ) : null}
    </div>
  );
}

/**
 * One of the agent's files, edited in place in a box that grows with its
 * text. Saves on blur over the version it opened, so a change the agent made
 * meanwhile is never overwritten without asking.
 */
export function FileField({
  monoId,
  file,
  value,
  hash,
  label,
  author,
  placeholder,
  autoFocus = false,
}: {
  monoId: string;
  file: MonoFile;
  value: string;
  hash: string;
  label: string;
  /** Who else writes the file, named when it changed under the editor. */
  author: string;
  placeholder: string;
  autoFocus?: boolean;
}) {
  const [draft, setDraft] = useState(value);
  const [conflict, setConflict] = useState(false);
  // The version the draft started from; a newer one replaces a clean draft.
  const base = useRef({ value, hash });
  if (base.current.hash !== hash && draft === base.current.value) {
    base.current = { value, hash };
    setDraft(value);
  }
  const save = async (overwrite = false) => {
    const text = draft;
    try {
      const next = await saveMonoFile(
        monoId,
        file,
        text,
        overwrite ? undefined : base.current.hash,
      );
      base.current = { value: text, hash: next };
      setConflict(false);
    } catch (error) {
      if (error instanceof MonoFileConflict) setConflict(true);
      else console.warn(`Could not save ${label}`, error);
    }
  };
  return (
    <>
      <MarkdownSourceEditor
        lineNumbers={false}
        className="min-h-full"
        label={label}
        value={draft}
        autoFocus={autoFocus}
        placeholder={placeholder}
        onChange={setDraft}
        onBlur={() => {
          if (draft !== base.current.value) void save();
        }}
      />
      {conflict ? (
        <div className="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1 px-2 text-[11px] leading-4 text-content/60">
          <span className="min-w-0 flex-1">
            {author} changed this while you were editing.
          </span>
          <button
            type="button"
            onClick={() => {
              setConflict(false);
              setDraft(value);
              base.current = { value, hash };
            }}
            className="rounded px-1.5 py-0.5 text-content/70 hover:bg-content/8 hover:text-content"
          >
            Use theirs
          </button>
          <button
            type="button"
            onClick={() => void save(true)}
            className="rounded px-1.5 py-0.5 text-content/70 hover:bg-content/8 hover:text-content"
          >
            Keep mine
          </button>
        </div>
      ) : null}
    </>
  );
}

/** How much of MEMORY.md loads, against the budget each turn has for it. */
export function MemoryGauge({ memory }: { memory: string }) {
  const budget = memoryWithinBudget(memory);
  const share = Math.max(
    budget.lines / MEMORY_MAX_LINES,
    budget.bytes / MEMORY_MAX_BYTES,
  );
  const tone = budget.droppedLines
    ? "bg-red-500/70"
    : share >= 0.8
      ? "bg-amber-500/70"
      : "bg-content/30";
  return (
    <div className="mt-2 flex items-center gap-2 px-2 text-[11px] leading-4 text-content/45">
      <span className="h-1 flex-1 overflow-hidden rounded-full bg-content/8">
        <span
          className={`block h-full rounded-full ${tone}`}
          style={{ width: `${Math.min(100, share * 100)}%` }}
        />
      </span>
      <span className="shrink-0 tabular-nums">
        {budget.droppedLines
          ? `${budget.droppedLines} lines not loading`
          : `${budget.lines} / ${MEMORY_MAX_LINES} lines`}
      </span>
    </div>
  );
}

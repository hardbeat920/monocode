import { invoke } from "@tauri-apps/api/core";
import { copyText } from "../../../platform/tauri/clipboard";
import {
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type Ref,
  type TextareaHTMLAttributes,
} from "react";
import { ChevronLeft, type IconComponent } from "../../../shared/ui/icons";
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
import { playCue } from "../../settings/model/sounds";
import {
  MEMORY_MAX_BYTES,
  MEMORY_MAX_LINES,
  memoryWithinBudget,
  loadMonoFiles,
  MonoFileConflict,
  saveMonoFile,
  type MonoFile,
} from "../model/monoFiles";

/**
 * The top of a page in the Mono's panel: back, a title, and its actions.
 * Without `onBack` it heads a section of the settings dialog instead.
 */
export function PageHeader({
  title,
  onBack,
  children,
}: {
  title: ReactNode;
  onBack?: () => void;
  children?: ReactNode;
}) {
  return (
    // Same height as the panel's own header, so a page does not jump.
    <header
      className={`flex h-10 shrink-0 items-center gap-1 border-b border-stroke ${
        onBack ? "pl-2" : "pl-4"
      }`}
      style={{
        paddingRight: "calc(0.75rem + var(--mono-window-controls-width, 0px))",
      }}
      data-tauri-drag-region="deep"
    >
      {onBack ? (
        <IconButton label="Back" onClick={onBack}>
          <ChevronLeft className="size-3.5" strokeWidth={1.75} />
        </IconButton>
      ) : null}
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

/** A setting that is on or off: its name and what it does, then a switch. */
export function SwitchRow({
  label,
  description,
  on,
  onChange,
  disabled = false,
}: {
  label: string;
  /** Clamped to one line under the label; the full text is its tooltip. */
  description: string;
  on: boolean;
  onChange: (on: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <div
      title={description}
      className={`flex items-center gap-3 px-3 py-2 ${disabled ? "opacity-40" : ""}`}
    >
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="text-[13px] leading-5 text-content/90">{label}</span>
        <span className="line-clamp-1 text-[12px] leading-5 text-content/40">
          {description}
        </span>
      </span>
      <button
        type="button"
        role="switch"
        aria-label={label}
        aria-checked={on}
        disabled={disabled}
        onClick={() => {
          onChange(!on);
          playCue("switch");
        }}
        className={`relative h-5 w-9 shrink-0 rounded-full transition-colors disabled:cursor-not-allowed ${
          on ? "bg-accent" : "bg-content/20"
        }`}
      >
        <span
          className={`absolute top-0.5 size-4 rounded-full bg-white transition-[left] ${
            on ? "left-4.5" : "left-0.5"
          }`}
        />
      </button>
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

/**
 * An empty tab: its icon over a line on how it fills, centered in the space
 * the list would take.
 */
export function EmptyState({
  icon: Icon,
  children,
}: {
  icon: IconComponent;
  children: ReactNode;
}) {
  return (
    <div className="flex min-h-full flex-col items-center justify-center gap-3 px-6 py-10 text-center">
      <Icon
        className="size-4.5 text-content/55"
        strokeWidth={1.75}
        aria-hidden
      />
      <p className="max-w-72 text-[12px] leading-4.5 text-content/55 text-balance">
        {children}
      </p>
    </div>
  );
}

export function MascotPicker({
  current,
  color,
  onPick,
  columns = 10,
}: {
  current: string;
  color: string;
  onPick: (mascot: string) => void;
  /** Five lays them out as larger tiles, for a popover of their own. */
  columns?: 5 | 10;
}) {
  const large = columns === 5;
  return (
    <div
      role="radiogroup"
      aria-label="Mascot"
      className={`grid gap-0.5 px-1 ${large ? "grid-cols-5 gap-1" : "grid-cols-10"}`}
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
            className={`grid min-w-0 place-items-center ${
              large ? "h-12 rounded-lg" : "h-8 rounded-md"
            } ${
              selected
                ? "bg-selection ring-1 ring-content/20"
                : "hover:bg-content/6"
            }`}
          >
            <PixelMascot
              name={name}
              color={color}
              still={!selected}
              className={large ? "size-8" : "size-5"}
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
  columns = 10,
}: {
  current: string;
  onPick: (color: string) => void;
  /** Opens the custom picker in place, for use inside another popover. */
  inline?: boolean;
  /** Five lays them out as larger swatches, under the larger mascots. */
  columns?: 5 | 10;
}) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLDivElement>(null);
  const presetIndex = MONO_COLORS.findIndex((color) => color === current);
  return (
    <div ref={anchor} role="group" aria-label="Color">
      <ColorSwatchRow
        className={`grid place-items-center px-1 ${
          columns === 5 ? "grid-cols-5 gap-y-2" : "grid-cols-10 gap-0.5"
        }`}
        size={columns === 5 ? "lg" : "sm"}
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

async function recoveryCommand<T>(
  command: string,
  args: Record<string, unknown>,
): Promise<T> {
  let timer = 0;
  try {
    return await Promise.race([
      invoke<T>(command, args),
      new Promise<never>((_resolve, reject) => {
        timer = window.setTimeout(
          () => reject(new Error("Timed out; the draft is still open")),
          8000,
        );
      }),
    ]);
  } finally {
    window.clearTimeout(timer);
  }
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
  saveRef,
  onRecoverClose,
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
  saveRef?: Ref<() => Promise<boolean>>;
  onRecoverClose?: (path: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  const [conflict, setConflict] = useState(false);
  // The version the draft started from; a newer one replaces a clean draft.
  const base = useRef({ value, hash });
  const seenHash = useRef(hash);
  const uncertain = useRef(false);
  const [uncertainSave, setUncertainSave] = useState(false);
  const mounted = useRef(false);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  if (
    !uncertain.current &&
    seenHash.current !== hash &&
    draft === base.current.value
  ) {
    seenHash.current = hash;
    base.current = { value, hash };
    setDraft(value);
  }
  const draftRef = useRef(draft);
  draftRef.current = draft;
  const pending = useRef<{ id: string; promise: Promise<boolean> } | null>(
    null,
  );
  const unconfirmed = useRef(new Set<string>());
  const generation = useRef(0);
  const cancelFlushes = useRef(new Set<() => void>());
  const recovering = useRef(false);
  const [slow, setSlow] = useState(false);
  const [recovery, setRecovery] = useState<{ text: string; path: string }>();
  const recoveryRef = useRef<{ text: string; path: string } | undefined>(
    undefined,
  );
  const [recoveryError, setRecoveryError] = useState<string>();
  const [recoveryBusy, setRecoveryBusy] = useState(false);
  const [pathCopied, setPathCopied] = useState(false);
  const blocked = useRef(false);
  const [error, setError] = useState(false);
  const fence = async (ids: string[]) => {
    for (const id of ids) {
      await recoveryCommand("mono_cancel_soul_save", {
        mono: monoId,
        requestId: id,
      });
      unconfirmed.current.delete(id);
    }
  };
  const save = (
    overwrite = false,
    retryUncertain = false,
  ): Promise<boolean> => {
    if (recovering.current) return Promise.resolve(false);
    if (pending.current) return pending.current.promise;
    if (uncertain.current && !overwrite && !retryUncertain) {
      setConflict(true);
      return Promise.resolve(false);
    }
    if (
      !uncertain.current &&
      draftRef.current === base.current.value &&
      !unconfirmed.current.size
    )
      return Promise.resolve(true);
    if (
      blocked.current &&
      !overwrite &&
      draftRef.current !== base.current.value
    )
      return Promise.resolve(false);
    const text = draftRef.current;
    const attempt = generation.current;
    const id = crypto.randomUUID();
    const previous = [...unconfirmed.current];
    unconfirmed.current.add(id);
    const request = (async () => {
      let invoked = false;
      try {
        await fence(previous);
        if (attempt !== generation.current) return false;
        if (!uncertain.current && text === base.current.value) {
          unconfirmed.current.delete(id);
          return true;
        }
        const expectedHash = overwrite
          ? (await loadMonoFiles(monoId))[`${file}Hash`]
          : base.current.hash;
        if (attempt !== generation.current) return false;
        invoked = true;
        const next = await saveMonoFile(monoId, file, text, expectedHash, id);
        unconfirmed.current.delete(id);
        if (attempt !== generation.current) return false;
        base.current = { value: text, hash: next };
        uncertain.current = false;
        setUncertainSave(false);
        blocked.current = false;
        setConflict(false);
        setError(false);
        return true;
      } catch (error) {
        if (attempt !== generation.current) return false;
        blocked.current = true;
        if (error instanceof MonoFileConflict) {
          unconfirmed.current.delete(id);
          setConflict(true);
        } else {
          if (invoked) {
            uncertain.current = true;
            setUncertainSave(true);
          }
          setError(true);
          console.warn(`Could not save ${label}`, error);
        }
        return false;
      }
    })();
    pending.current = { id, promise: request };
    const timer = window.setTimeout(() => setSlow(true), 5000);
    void request.then(() => {
      window.clearTimeout(timer);
      if (pending.current?.promise === request) {
        pending.current = null;
        if (mounted.current) {
          setSlow(false);
          if (uncertain.current) setConflict(true);
        }
      }
    });
    return request;
  };
  useImperativeHandle(saveRef, () => async () => {
    if (recovering.current) return false;
    const attempt = generation.current;
    let cancel!: () => void;
    const cancelled = new Promise<boolean>((resolve) => {
      cancel = () => resolve(false);
    });
    cancelFlushes.current.add(cancel);
    try {
      do {
        const saved = await Promise.race([save(), cancelled]);
        if (attempt !== generation.current || recovering.current) return false;
        if (!saved) return false;
      } while (uncertain.current || draftRef.current !== base.current.value);
      return true;
    } finally {
      cancelFlushes.current.delete(cancel);
    }
  });
  const saveRecovery = async () => {
    const text = draftRef.current;
    setRecoveryBusy(true);
    setRecoveryError(undefined);
    try {
      const path = await recoveryCommand<string>("mono_save_soul_recovery", {
        mono: monoId,
        text,
      });
      if (draftRef.current !== text) {
        setRecoveryError(
          "The draft changed while saving the copy. Save another copy before closing.",
        );
        return;
      }
      setPathCopied(false);
      recoveryRef.current = { text, path };
      setRecovery(recoveryRef.current);
    } catch (error) {
      setRecoveryError(`Could not save a recovery copy: ${String(error)}`);
    } finally {
      setRecoveryBusy(false);
    }
  };
  const closeWithRecovery = async () => {
    if (!recovery || recovery.text !== draftRef.current || recovering.current)
      return;
    recovering.current = true;
    setRecoveryBusy(true);
    if (unconfirmed.current.size) {
      uncertain.current = true;
      setUncertainSave(true);
    }
    generation.current++;
    for (const cancel of cancelFlushes.current) cancel();
    try {
      await fence([...unconfirmed.current]);
      if (recovery.text !== draftRef.current) {
        setRecoveryError(
          "The draft changed. Save another recovery copy before closing.",
        );
        recoveryRef.current = undefined;
        setRecovery(undefined);
        return;
      }
      onRecoverClose?.(recovery.path);
    } catch (error) {
      setRecoveryError(`Could not stop waiting safely: ${String(error)}`);
    } finally {
      recovering.current = false;
      setRecoveryBusy(false);
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
        onChange={(text) => {
          draftRef.current = text;
          setDraft(text);
          recoveryRef.current = undefined;
          setRecovery(undefined);
          setPathCopied(false);
        }}
        onBlur={() => {
          if (draftRef.current !== base.current.value) void save();
        }}
      />
      {onRecoverClose && (slow || error || conflict || recovery) ? (
        <div
          className="space-y-2 px-3 pb-3 text-xs text-content/70"
          role="status"
        >
          <p>
            The Soul save has not been confirmed. You can save a separate
            recovery copy; the earlier save may still finish.
          </p>
          <button
            type="button"
            disabled={recoveryBusy}
            onClick={() => void saveRecovery()}
            className="rounded px-1.5 py-0.5 hover:bg-content/8"
          >
            Save recovery copy
          </button>
          <button
            type="button"
            onClick={() =>
              void copyText(draftRef.current).catch((error) =>
                setRecoveryError(`Could not copy draft: ${String(error)}`),
              )
            }
            className="rounded px-1.5 py-0.5 hover:bg-content/8"
          >
            Copy draft
          </button>
          {recovery ? (
            <div>
              <p>
                Recovery copy: <code>{recovery.path}</code>
              </p>
              <p>Keep this file; it will not be removed automatically.</p>
              <button
                type="button"
                onClick={() => {
                  const path = recovery.path;
                  void copyText(path)
                    .then(() => {
                      if (recoveryRef.current?.path === path)
                        setPathCopied(true);
                    })
                    .catch((error) =>
                      setRecoveryError(
                        `Could not copy the recovery path: ${String(error)}`,
                      ),
                    );
                }}
                className="rounded px-1.5 py-0.5 hover:bg-content/8"
              >
                {pathCopied ? "Recovery path copied" : "Copy recovery path"}
              </button>
              <button
                type="button"
                onClick={() => setPathCopied(true)}
                className="rounded px-1.5 py-0.5 hover:bg-content/8"
              >
                I have recorded the recovery path
              </button>
              <button
                type="button"
                disabled={recoveryBusy || !pathCopied}
                onClick={() => void closeWithRecovery()}
                className="rounded px-1.5 py-0.5 hover:bg-content/8"
              >
                Close editor and keep recovery copy
              </button>
            </div>
          ) : null}
          {recoveryError ? <p role="alert">{recoveryError}</p> : null}
        </div>
      ) : null}
      {error ? (
        <div role="alert" className="px-2 text-xs text-red-400">
          Could not save.{" "}
          <button
            type="button"
            onClick={() => {
              blocked.current = false;
              void save(false, true);
            }}
          >
            Retry
          </button>
        </div>
      ) : null}
      {conflict ? (
        <div className="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1 px-2 text-[11px] leading-4 text-content/60">
          <span className="min-w-0 flex-1">
            {uncertainSave
              ? "An earlier save may still finish. Retry saving or keep a recovery copy."
              : `${author} changed this while you were editing.`}
          </span>
          <button
            type="button"
            disabled={uncertainSave}
            onClick={() => {
              const original = draftRef.current;
              void loadMonoFiles(monoId)
                .then((files) => {
                  if (draftRef.current !== original) return;
                  const latest = {
                    value: files[file],
                    hash: files[`${file}Hash`],
                  };
                  seenHash.current = hash;
                  base.current = latest;
                  draftRef.current = latest.value;
                  setDraft(latest.value);
                  blocked.current = false;
                  uncertain.current = false;
                  setUncertainSave(false);
                  setConflict(false);
                })
                .catch((error) => {
                  setError(true);
                  console.warn(`Could not reload ${label}`, error);
                });
            }}
            className="rounded px-1.5 py-0.5 text-content/70 hover:bg-content/8 hover:text-content"
          >
            Use theirs
          </button>
          <button
            type="button"
            disabled={uncertainSave && !!pending.current}
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

import {
  ChevronDown,
  Lock,
  Pencil,
  Shield,
  Sparkles,
} from "../../../shared/ui/icons";
import {
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import {
  RUNTIME_MODE_HINT,
  RUNTIME_MODE_LABEL,
  type HarnessId,
  type RuntimeMode,
} from "../model/session";
import {
  coerceRuntimeMode,
  getModelSnapshot,
  runtimeModesFor,
  subscribeModels,
} from "../model/models";
import { Popover } from "../../../shared/ui/Popover";

type Props = {
  harness: HarnessId;
  model: string;
  value: RuntimeMode;
  onChange: (mode: RuntimeMode) => void;
  onClose?: () => void;
  busy?: boolean;
  side?: "top" | "bottom";
  variant?: "pill" | "plain";
};

const MENU_WIDTH = 288;

const ICONS: Record<RuntimeMode, typeof Lock> = {
  supervised: Lock,
  "auto-accept-edits": Pencil,
  auto: Sparkles,
  "full-access": Shield,
};

export function AccessPicker({
  harness,
  model,
  value: requested,
  onChange,
  onClose,
  busy = false,
  side = "top",
  variant = "pill",
}: Props) {
  // Re-render when a late catalog discovery changes which modes a model offers.
  useSyncExternalStore(subscribeModels, getModelSnapshot, getModelSnapshot);
  const modes = runtimeModesFor(harness, model);
  const value = coerceRuntimeMode(harness, model, requested);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  // A model that can't do the chosen mode (e.g. Auto) steps down to the closest
  // preceding mode it supports.
  useEffect(() => {
    if (value !== requested) onChangeRef.current(value);
  }, [value, requested]);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(() => Math.max(0, modes.indexOf(value)));
  const root = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const Icon = ICONS[value];

  const dismiss = (restore: boolean) => {
    setOpen(false);
    if (restore) onCloseRef.current?.();
  };

  const modesKey = modes.join();
  useEffect(() => {
    if (!open) return;
    setActive(Math.max(0, modes.indexOf(value)));
    // `modes` is derived from `modesKey`, which keeps this from re-running
    // every render while still following changes to the offered modes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, value, modesKey]);

  const pick = (mode: RuntimeMode) => {
    onChange(mode);
    dismiss(true);
  };

  const onMenuKey = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((i) => Math.min(modes.length - 1, i + 1));
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((i) => Math.max(0, i - 1));
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      const mode = modes[active];
      if (mode) pick(mode);
    }
  };

  return (
    <div ref={root} className="relative">
      <button
        type="button"
        data-access-picker-trigger
        title={`${RUNTIME_MODE_HINT[value]}${busy ? " Changes apply to the next turn." : ""}`}
        aria-label={RUNTIME_MODE_LABEL[value]}
        aria-expanded={open}
        aria-haspopup="listbox"
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => {
          if (open) {
            dismiss(true);
            return;
          }
          setOpen(true);
        }}
        className={
          variant === "plain"
            ? `-mx-1.5 flex h-7 max-w-52 items-center gap-2 rounded-md px-1.5 text-[12px] text-content/85 ${
                open ? "bg-content/8" : "hover:bg-content/6"
              }`
            : `flex h-6.5 max-w-52 items-center gap-1 rounded-md px-1.5 ${
                open
                  ? "bg-selection text-content"
                  : "bg-selection text-content hover:bg-selection-hover"
              }`
        }
      >
        <Icon
          className={`size-3.5 shrink-0 ${value === "full-access" ? "text-amber-400/90" : ""}`}
          strokeWidth={1.75}
        />
        <span
          className={`min-w-0 truncate ${variant === "plain" ? "" : "text-[11px]"}`}
        >
          {RUNTIME_MODE_LABEL[value]}
        </span>
        <ChevronDown
          className={`size-3 shrink-0 text-content/50 ${open ? "rotate-180" : ""}`}
          strokeWidth={1.75}
        />
      </button>
      {open ? (
        <Popover
          anchor={root}
          side={side}
          width={MENU_WIDTH}
          autoFocus
          onDismiss={(reason) => dismiss(reason === "escape")}
          role="listbox"
          aria-label="Access"
          data-access-picker
          tabIndex={-1}
          onKeyDown={onMenuKey}
          className="p-1"
        >
          {modes.map((mode, index) => {
            const ModeIcon = ICONS[mode];
            const selected = mode === value;
            const highlighted = index === active;
            return (
              <button
                key={mode}
                type="button"
                role="option"
                aria-selected={selected}
                onMouseDown={(e) => e.preventDefault()}
                onMouseEnter={() => setActive(index)}
                onClick={() => pick(mode)}
                className={`flex w-full items-start gap-2.5 rounded-lg px-2 py-2 text-left ${
                  highlighted || selected
                    ? "bg-selection text-content"
                    : "text-content hover:bg-content/5"
                }`}
              >
                <ModeIcon
                  className={`mt-0.5 size-3.5 shrink-0 ${mode === "full-access" ? "text-amber-400/90" : "text-content/70"}`}
                  strokeWidth={1.75}
                />
                <span className="min-w-0">
                  <span className="block text-[13px] font-medium leading-5">
                    {RUNTIME_MODE_LABEL[mode]}
                  </span>
                  <span className="mt-0.5 block text-[11px] leading-4 text-content/50">
                    {RUNTIME_MODE_HINT[mode]}
                  </span>
                </span>
              </button>
            );
          })}
          {busy ? (
            <p className="px-2 py-1.5 text-[11px] leading-4 text-content/50">
              Access changes apply to the next turn. Stop and resend to apply
              them now.
            </p>
          ) : null}
        </Popover>
      ) : null}
    </div>
  );
}

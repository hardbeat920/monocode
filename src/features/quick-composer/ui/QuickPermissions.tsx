import { useEffect, useId, useRef, useState } from "react";
import {
  Check,
  Lock,
  Pencil,
  Shield,
  Sparkles,
} from "../../../shared/ui/icons";
import {
  RUNTIME_MODES,
  RUNTIME_MODE_HINT,
  RUNTIME_MODE_LABEL,
  type RuntimeMode,
} from "../../sessions/model/session";

const ICONS = {
  supervised: Lock,
  "auto-accept-edits": Pencil,
  auto: Sparkles,
  "full-access": Shield,
};

export function QuickPermissionIcon({
  mode,
  className,
}: {
  mode: RuntimeMode;
  className?: string;
}) {
  const Icon = ICONS[mode];
  return (
    <Icon
      className={`${className ?? ""} ${mode === "full-access" ? "text-amber-400/90" : ""}`}
      strokeWidth={1.75}
    />
  );
}

export function QuickPermissions({
  value,
  onChange,
  onClose,
  embedded = false,
}: {
  value: RuntimeMode;
  onChange: (mode: RuntimeMode) => void;
  onClose: () => void;
  embedded?: boolean;
}) {
  const root = useRef<HTMLDivElement>(null);
  const id = useId();
  const [active, setActive] = useState(RUNTIME_MODES.indexOf(value));
  useEffect(() => {
    if (!embedded) root.current?.focus({ preventScroll: true });
  }, [embedded]);
  const pick = (mode: RuntimeMode) => {
    onChange(mode);
    if (!embedded) onClose();
  };

  return (
    <div
      ref={root}
      role="listbox"
      aria-label="Permissions"
      aria-activedescendant={`${id}-${active}`}
      tabIndex={embedded ? 0 : -1}
      className={`min-h-0 overflow-y-auto overscroll-none p-2 outline-none ${embedded ? "" : "border-t border-stroke"}`}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          onClose();
        } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          const step = event.key === "ArrowDown" ? 1 : -1;
          setActive(
            (index) =>
              (index + step + RUNTIME_MODES.length) % RUNTIME_MODES.length,
          );
        } else if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          pick(RUNTIME_MODES[active]);
        }
      }}
    >
      {RUNTIME_MODES.map((mode, index) => (
        <button
          key={mode}
          id={`${id}-${index}`}
          type="button"
          role="option"
          tabIndex={-1}
          aria-selected={value === mode}
          onMouseDown={(event) => {
            event.preventDefault();
            root.current?.focus({ preventScroll: true });
          }}
          onMouseEnter={() => setActive(index)}
          onClick={() => pick(mode)}
          className={`flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left ${(embedded ? value === mode : active === index) ? "bg-selection-emphasis text-content" : "text-content/75 hover:bg-selection-hover"}`}
        >
          <QuickPermissionIcon mode={mode} className="size-4 shrink-0" />
          <span className="min-w-0 flex-1">
            <span className="block text-[13px] font-medium">
              {RUNTIME_MODE_LABEL[mode]}
            </span>
            <span className="mt-0.5 block text-[11px] text-content/45">
              {RUNTIME_MODE_HINT[mode]}
            </span>
          </span>
          {value === mode ? (
            <Check className="size-3.5 shrink-0 text-accent" />
          ) : null}
        </button>
      ))}
    </div>
  );
}

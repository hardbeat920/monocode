import {
  useMemo,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { Check } from "./icons";
import { Popover } from "./Popover";

export type ExplorerMenuItem =
  | { kind: "sep" }
  | {
      kind: "item";
      id: string;
      label: string;
      shortcut?: string;
      disabled?: boolean;
      danger?: boolean;
      checked?: boolean;
    };

type Props = {
  x: number;
  y: number;
  items: ExplorerMenuItem[];
  ariaLabel?: string;
  header?: ReactNode;
  width?: number;
  onPick: (id: string) => void;
  onClose: () => void;
};

const MENU_WIDTH = 228;

function itemIndexAt(items: ExplorerMenuItem[], start: number, dir: 1 | -1): number {
  let i = start;
  while (i >= 0 && i < items.length) {
    const item = items[i];
    if (item?.kind === "item") return i;
    i += dir;
  }
  return start;
}

export function ExplorerMenu({
  x,
  y,
  items,
  ariaLabel = "File actions",
  header,
  width = MENU_WIDTH,
  onPick,
  onClose,
}: Props) {
  const [active, setActive] = useState(() => itemIndexAt(items, 0, 1));

  const ids = useMemo(
    () =>
      items.flatMap((item, index) =>
        item.kind === "item" ? [{ index, id: item.id, disabled: !!item.disabled }] : [],
      ),
    [items],
  );

  const move = (dir: 1 | -1) => {
    const from = ids.findIndex((item) => item.index === active);
    const next = ids[(from + dir + ids.length) % ids.length];
    if (next) setActive(next.index);
  };

  const onMenuKey = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      move(1);
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      move(-1);
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      const item = items[active];
      if (item?.kind === "item" && !item.disabled) onPick(item.id);
    }
  };

  return (
    <Popover
      anchor={{ x, y }}
      gap={0}
      width={width}
      autoFocus
      onDismiss={onClose}
      role="menu"
      tabIndex={-1}
      aria-label={ariaLabel}
      onKeyDown={onMenuKey}
      onContextMenu={(e) => e.preventDefault()}
      className="overflow-y-auto overscroll-none p-1"
    >
      {header ? (
        <>
          {header}
          <div role="separator" className="my-1 h-px bg-content/10" />
        </>
      ) : null}
      {items.map((item, index) => {
        if (item.kind === "sep") {
          return (
            <div
              key={`sep-${index}`}
              role="separator"
              className="my-1 h-px bg-content/10"
            />
          );
        }
        const highlighted = index === active;
        return (
          <button
            key={item.id}
            type="button"
            role={item.checked == null ? "menuitem" : "menuitemcheckbox"}
            aria-checked={item.checked}
            disabled={item.disabled}
            onMouseDown={(e) => e.preventDefault()}
            onMouseEnter={() => setActive(index)}
            onClick={() => {
              if (!item.disabled) onPick(item.id);
            }}
            className={`flex h-7 w-full items-center gap-3 rounded-lg px-2 text-left text-[13px] leading-none ${
              item.disabled
                ? "text-content/30"
                : item.danger
                  ? highlighted
                    ? "bg-red-500/20 text-red-300"
                    : "text-red-300/90 hover:bg-red-500/15"
                  : highlighted
                    ? "bg-content/10 text-content"
                    : "text-content hover:bg-content/5"
            }`}
          >
            <span className="min-w-0 flex-1 truncate">{item.label}</span>
            {item.checked ? (
              <Check className="size-3.5 shrink-0" strokeWidth={2.25} />
            ) : item.shortcut ? (
              <span className="shrink-0 text-[11px] text-content/40">
                {item.shortcut}
              </span>
            ) : null}
          </button>
        );
      })}
    </Popover>
  );
}

import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { LAYER } from "../lib/layers";
import { Check, ChevronRight } from "./icons";
import { Popover } from "./Popover";

type MenuAction = {
  kind: "item";
  id: string;
  label: string;
  shortcut?: string;
  disabled?: boolean;
  danger?: boolean;
  checked?: boolean;
};

export type ExplorerMenuItem =
  { kind: "sep" } | (MenuAction & { submenu?: MenuAction[] });

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

function itemIndexAt(
  items: ExplorerMenuItem[],
  start: number,
  dir: 1 | -1,
): number {
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
  const menuId = useId();
  const menuRef = useRef<HTMLDivElement>(null);
  const submenuRef = useRef<HTMLDivElement>(null);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [active, setActive] = useState(() => itemIndexAt(items, 0, 1));
  const [submenu, setSubmenu] = useState<{
    index: number;
    anchor: HTMLButtonElement;
  } | null>(null);
  const [submenuActive, setSubmenuActive] = useState(-1);
  const submenuItem = submenu ? items[submenu.index] : undefined;
  const submenuItems =
    submenuItem?.kind === "item" ? submenuItem.submenu : undefined;

  const cancelClose = () => {
    if (closeTimer.current != null) clearTimeout(closeTimer.current);
    closeTimer.current = null;
  };

  const closeSubmenu = () => {
    cancelClose();
    if (submenuRef.current?.contains(document.activeElement)) {
      menuRef.current?.focus();
    }
    setSubmenu(null);
    setSubmenuActive(-1);
  };

  const scheduleClose = () => {
    cancelClose();
    closeTimer.current = setTimeout(closeSubmenu, 180);
  };

  useEffect(() => cancelClose, []);

  const ids = useMemo(
    () =>
      items.flatMap((item, index) =>
        item.kind === "item"
          ? [{ index, id: item.id, disabled: !!item.disabled }]
          : [],
      ),
    [items],
  );

  const move = (dir: 1 | -1) => {
    const from = ids.findIndex((item) => item.index === active);
    const next = ids[(from + dir + ids.length) % ids.length];
    if (next) setActive(next.index);
  };

  const onMenuKey = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (submenuItems) {
      if (e.key === "ArrowLeft") {
        e.preventDefault();
        e.stopPropagation();
        closeSubmenu();
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const direction = e.key === "ArrowDown" ? 1 : -1;
        setSubmenuActive((current) =>
          current < 0
            ? direction === 1
              ? 0
              : submenuItems.length - 1
            : (current + direction + submenuItems.length) % submenuItems.length,
        );
      } else if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        const item = submenuItems[submenuActive];
        if (item && !item.disabled) onPick(item.id);
      }
      return;
    }
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
    if (e.key === "Enter" || e.key === " " || e.key === "ArrowRight") {
      e.preventDefault();
      const item = items[active];
      if (item?.kind !== "item" || item.disabled) return;
      if (item.submenu?.length) {
        const anchor = menuRef.current?.querySelector<HTMLButtonElement>(
          `[data-menu-index="${active}"]`,
        );
        if (anchor) {
          cancelClose();
          setSubmenu({ index: active, anchor });
          setSubmenuActive(0);
        }
      } else if (e.key !== "ArrowRight") {
        onPick(item.id);
      }
    }
  };

  const renderItem = (
    item: MenuAction & { submenu?: MenuAction[] },
    index: number,
    inSubmenu = false,
  ) => {
    const highlighted = index === (inSubmenu ? submenuActive : active);
    const hasSubmenu = !!item.submenu?.length;
    return (
      <button
        key={item.id}
        id={`${menuId}-${inSubmenu ? "sub-" : ""}${index}`}
        data-menu-index={index}
        type="button"
        role={item.checked == null ? "menuitem" : "menuitemcheckbox"}
        aria-checked={item.checked}
        aria-haspopup={hasSubmenu ? "menu" : undefined}
        aria-expanded={hasSubmenu ? submenu?.index === index : undefined}
        aria-controls={
          hasSubmenu && submenu?.index === index
            ? `${menuId}-submenu`
            : undefined
        }
        disabled={item.disabled}
        onMouseDown={(e) => e.preventDefault()}
        onMouseEnter={(e) => {
          cancelClose();
          if (inSubmenu) {
            setSubmenuActive(index);
            submenuRef.current?.focus();
            return;
          }
          setActive(index);
          if (hasSubmenu && !item.disabled) {
            setSubmenu({ index, anchor: e.currentTarget });
            setSubmenuActive(-1);
          } else {
            closeSubmenu();
          }
        }}
        onClick={(e) => {
          if (item.disabled) return;
          if (hasSubmenu) {
            cancelClose();
            setSubmenu({ index, anchor: e.currentTarget });
            setSubmenuActive(0);
            submenuRef.current?.focus();
          } else {
            onPick(item.id);
          }
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
        {hasSubmenu ? (
          <ChevronRight
            className="size-3.5 shrink-0 text-content/50"
            strokeWidth={1.75}
          />
        ) : item.checked ? (
          <Check className="size-3.5 shrink-0" strokeWidth={2.25} />
        ) : item.shortcut ? (
          <span className="shrink-0 text-[11px] text-content/40">
            {item.shortcut}
          </span>
        ) : null}
      </button>
    );
  };

  return (
    <>
      <Popover
        ref={menuRef}
        anchor={{ x, y }}
        gap={0}
        width={width}
        autoFocus
        onDismiss={(reason) => {
          if (reason === "escape" && submenu) closeSubmenu();
          else onClose();
        }}
        ignore={`[data-explorer-menu="${menuId}"]`}
        data-explorer-menu={menuId}
        role="menu"
        tabIndex={-1}
        aria-label={ariaLabel}
        aria-activedescendant={`${menuId}-${active}`}
        onKeyDown={onMenuKey}
        onContextMenu={(e) => e.preventDefault()}
        onMouseEnter={cancelClose}
        onMouseLeave={submenu ? scheduleClose : undefined}
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
          return renderItem(item, index);
        })}
      </Popover>
      {submenu && submenuItems && submenuItem?.kind === "item" ? (
        <Popover
          ref={submenuRef}
          id={`${menuId}-submenu`}
          anchor={submenu.anchor}
          side="right"
          gap={4}
          width={MENU_WIDTH}
          layer={LAYER.submenu}
          autoFocus
          role="menu"
          tabIndex={-1}
          aria-label={submenuItem.label}
          aria-activedescendant={
            submenuActive >= 0 ? `${menuId}-sub-${submenuActive}` : undefined
          }
          data-explorer-menu={menuId}
          onKeyDown={onMenuKey}
          onContextMenu={(e) => e.preventDefault()}
          onMouseEnter={cancelClose}
          onMouseLeave={scheduleClose}
          className="overflow-y-auto overscroll-none p-1"
        >
          {submenuItems.map((item, index) => renderItem(item, index, true))}
        </Popover>
      ) : null}
    </>
  );
}

import {
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { Check, ChevronDown, Search } from "./icons";
import { LAYER } from "../lib/layers";
import type { PopoverAlign } from "../lib/popover";
import { Popover } from "./Popover";

export type SearchableSelectOption = {
  value: string;
  label: string;
  keywords?: string;
};

export function SearchableSelect({
  label,
  value,
  options,
  onChange,
  placeholder = "Choose an option…",
  searchPlaceholder = "Search options…",
  emptyLabel = "No matching options",
  disabled = false,
  layer,
  variant = "field",
  searchable = true,
  align = "start",
}: {
  label: string;
  value: string;
  options: readonly SearchableSelectOption[];
  onChange: (value: string) => void;
  placeholder?: string;
  searchPlaceholder?: string;
  emptyLabel?: string;
  disabled?: boolean;
  layer?: number;
  variant?: "field" | "row" | "panel" | "pill";
  searchable?: boolean;
  align?: PopoverAlign;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [menuWidth, setMenuWidth] = useState<number>();
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const activeOption = useRef<HTMLButtonElement>(null);
  const listId = useId();
  const selected = options.find((option) => option.value === value);
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const filtered = useMemo(
    () =>
      normalizedQuery
        ? options.filter((option) =>
            `${option.label}\n${option.keywords ?? ""}`
              .toLocaleLowerCase()
              .includes(normalizedQuery),
          )
        : [...options],
    [normalizedQuery, options],
  );
  const activeId =
    filtered[active] != null ? `${listId}-option-${active}` : undefined;
  const popoverLayer =
    layer ??
    (root.current?.closest('[role="dialog"]')
      ? LAYER.dialogPopover
      : LAYER.popover);

  const close = (restoreFocus = false) => {
    setOpen(false);
    setQuery("");
    if (restoreFocus) trigger.current?.focus({ preventScroll: true });
  };

  const openMenu = () => {
    if (disabled) return;
    setMenuWidth(
      variant === "pill" || variant === "row"
        ? Math.max(240, root.current?.getBoundingClientRect().width ?? 0)
        : root.current?.getBoundingClientRect().width,
    );
    setQuery("");
    setActive(
      Math.max(
        0,
        options.findIndex((option) => option.value === value),
      ),
    );
    setOpen(true);
  };

  useEffect(() => {
    if (!open) return;
    const frame = requestAnimationFrame(() => {
      if (searchable) search.current?.focus({ preventScroll: true });
      else list.current?.focus({ preventScroll: true });
    });
    return () => cancelAnimationFrame(frame);
  }, [open, searchable]);

  useEffect(() => {
    if (!open) return;
    setActive((index) =>
      filtered.length === 0 ? 0 : Math.min(index, filtered.length - 1),
    );
  }, [filtered.length, open]);

  useLayoutEffect(() => {
    if (!open) return;
    const container = list.current;
    const item = activeOption.current;
    if (!container || !item) return;
    const bounds = container.getBoundingClientRect();
    const row = item.getBoundingClientRect();
    if (row.top < bounds.top) container.scrollTop -= bounds.top - row.top;
    else if (row.bottom > bounds.bottom) {
      container.scrollTop += row.bottom - bounds.bottom;
    }
  }, [active, open]);

  useEffect(() => {
    if (!disabled) return;
    setOpen(false);
    setQuery("");
  }, [disabled]);

  const pick = (next: string) => {
    onChange(next);
    close(true);
  };

  const onSearchKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      if (filtered.length > 0) {
        setActive((index) => Math.min(filtered.length - 1, index + 1));
      }
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      if (filtered.length > 0) {
        setActive((index) => Math.max(0, index - 1));
      }
      return;
    }
    if (event.key === "Home") {
      event.preventDefault();
      setActive(0);
      return;
    }
    if (event.key === "End") {
      event.preventDefault();
      setActive(Math.max(0, filtered.length - 1));
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      const option = filtered[active];
      if (!option) return;
      pick(option.value);
    }
  };

  const compact = variant === "pill" || variant === "row";
  const longMenu = searchable || options.length > 12;

  return (
    <div
      ref={root}
      className={
        variant === "pill" || variant === "row"
          ? "relative inline-flex max-w-56 shrink-0"
          : "relative min-w-0"
      }
    >
      <button
        ref={trigger}
        type="button"
        disabled={disabled}
        aria-label={`${label}: ${selected?.label ?? placeholder}`}
        aria-expanded={open}
        aria-haspopup="dialog"
        onClick={() => (open ? close() : openMenu())}
        onKeyDown={(event) => {
          if (open || (event.key !== "ArrowDown" && event.key !== "ArrowUp"))
            return;
          event.preventDefault();
          openMenu();
        }}
        className={
          variant === "row"
            ? "inline-flex h-7 max-w-full items-center gap-1 rounded-md bg-content/10 py-0 pr-1.5 pl-2 text-left text-[12px] outline-none hover:bg-content/[0.14] focus-visible:bg-content/[0.14] disabled:opacity-50"
            : variant === "panel"
              ? "flex h-14 w-full items-center justify-end gap-3 rounded-xl border border-content/6 bg-content/6 px-4 text-right text-[14px] font-medium outline-none hover:bg-content/8 focus:border-content/12 focus:bg-content/8 disabled:opacity-50 active:scale-[0.995]"
              : variant === "pill"
                ? "inline-flex h-7 max-w-full items-center gap-1 rounded-md bg-content/10 py-0 pr-1.5 pl-2 text-left text-[12px] outline-none hover:bg-content/[0.14] focus-visible:bg-content/[0.14] disabled:opacity-50"
                : "flex h-9 w-full items-center justify-between gap-2 rounded-md border border-content/10 bg-background-base px-2.5 text-left text-[13px] outline-none hover:border-content/20 focus:border-content/25 disabled:opacity-50 active:scale-[0.99]"
        }
      >
        <span
          className={`min-w-0 truncate ${variant === "panel" ? "flex-1 text-right" : variant === "pill" || variant === "row" ? "" : "flex-1"} ${selected ? "text-content" : "text-content/40"}`}
        >
          {selected?.label ?? placeholder}
        </span>
        <ChevronDown
          className={`shrink-0 text-content/45 transition-transform duration-150 ease-out ${variant === "pill" || variant === "row" ? "size-3" : "size-3.5"} ${open ? "rotate-180" : ""}`}
          strokeWidth={1.75}
        />
      </button>
      {open ? (
        <Popover
          anchor={root}
          side="bottom"
          align={align}
          gap={4}
          width={menuWidth}
          maxHeight={longMenu ? 240 : undefined}
          constrainHeight={longMenu}
          layer={popoverLayer}
          role="dialog"
          aria-label={`${label} options`}
          data-dialog-popover
          onDismiss={(reason) => close(reason === "escape")}
          className="flex flex-col overflow-hidden"
        >
          {searchable ? (
            <label className="flex h-8 shrink-0 items-center gap-2 border-b border-stroke px-2.5 text-content/45 focus-within:text-content/70">
              <Search className="size-3.5 shrink-0" strokeWidth={1.75} />
              <span className="sr-only">{searchPlaceholder}</span>
              <input
                ref={search}
                role="combobox"
                aria-label={searchPlaceholder}
                aria-expanded="true"
                aria-autocomplete="list"
                aria-controls={listId}
                aria-activedescendant={activeId}
                value={query}
                placeholder={searchPlaceholder}
                autoComplete="off"
                spellCheck={false}
                onChange={(event) => {
                  setQuery(event.target.value);
                  setActive(0);
                }}
                onKeyDown={onSearchKeyDown}
                className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/35"
              />
            </label>
          ) : null}
          <div
            ref={list}
            id={listId}
            role="listbox"
            aria-label={label}
            aria-activedescendant={searchable ? undefined : activeId}
            tabIndex={searchable ? undefined : 0}
            onKeyDown={searchable ? undefined : onSearchKeyDown}
            className="min-h-0 flex-1 overflow-y-auto overscroll-none p-1 outline-none"
          >
            {filtered.length > 0 ? (
              filtered.map((option, index) => {
                const highlighted = index === active;
                const isSelected = option.value === value;
                return (
                  <button
                    key={option.value}
                    ref={highlighted ? activeOption : undefined}
                    id={`${listId}-option-${index}`}
                    type="button"
                    role="option"
                    tabIndex={-1}
                    aria-selected={isSelected}
                    onMouseDown={(event) => event.preventDefault()}
                    onMouseEnter={() => setActive(index)}
                    onClick={() => pick(option.value)}
                    className={`flex w-full items-center gap-2 rounded-md px-2 text-left leading-none ${
                      compact
                        ? "h-7 text-[12px]"
                        : "h-8 text-[13px]"
                    } ${
                      highlighted
                        ? "bg-selection text-content"
                        : "text-content/75 hover:bg-content/5 hover:text-content"
                    }`}
                  >
                    <span className="grid size-3.5 shrink-0 place-items-center">
                      {isSelected ? (
                        <Check className="size-3" strokeWidth={2} />
                      ) : null}
                    </span>
                    <span className="min-w-0 flex-1 truncate">
                      {option.label}
                    </span>
                  </button>
                );
              })
            ) : (
              <p className="px-2 py-3 text-center text-[12px] text-content/45">
                {emptyLabel}
              </p>
            )}
          </div>
        </Popover>
      ) : null}
    </div>
  );
}

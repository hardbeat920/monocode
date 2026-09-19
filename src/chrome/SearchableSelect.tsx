import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { Check, ChevronDown, Search } from "./icons";
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
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [menuWidth, setMenuWidth] = useState<number>();
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const search = useRef<HTMLInputElement>(null);
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

  const close = (restoreFocus = false) => {
    setOpen(false);
    setQuery("");
    if (restoreFocus) trigger.current?.focus();
  };

  const openMenu = () => {
    if (disabled) return;
    setMenuWidth(root.current?.getBoundingClientRect().width);
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
    const frame = requestAnimationFrame(() => search.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    setActive((index) =>
      filtered.length === 0 ? 0 : Math.min(index, filtered.length - 1),
    );
  }, [filtered.length, open]);

  useEffect(() => {
    if (!open) return;
    activeOption.current?.scrollIntoView({ block: "nearest" });
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

  const onSearchKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
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

  return (
    <div ref={root} className="relative min-w-0">
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
        className="flex h-9 w-full items-center justify-between gap-2 rounded-md border border-content/10 bg-background-base px-2.5 text-left text-[13px] outline-none hover:border-content/20 focus:border-content/25 disabled:opacity-50 active:scale-[0.99]"
      >
        <span
          className={`min-w-0 flex-1 truncate ${selected ? "text-content" : "text-content/40"}`}
        >
          {selected?.label ?? placeholder}
        </span>
        <ChevronDown
          className={`size-3.5 shrink-0 text-content/45 transition-transform ${open ? "rotate-180" : ""}`}
          strokeWidth={1.75}
        />
      </button>
      {open ? (
        <Popover
          anchor={root}
          side="bottom"
          align="start"
          gap={4}
          width={menuWidth}
          maxHeight={300}
          layer={layer}
          role="dialog"
          aria-label={`${label} options`}
          data-dialog-popover
          onDismiss={(reason) => close(reason === "escape")}
          className="flex flex-col overflow-hidden"
        >
          <label className="flex h-11 shrink-0 items-center gap-2.5 border-b border-stroke px-3 text-content/45 focus-within:text-content/70">
            <Search className="size-4 shrink-0" strokeWidth={1.75} />
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
              className="min-w-0 flex-1 bg-transparent text-[13px] text-content outline-none placeholder:text-content/35"
            />
          </label>
          <div
            id={listId}
            role="listbox"
            aria-label={label}
            className="min-h-0 flex-1 overflow-y-auto overscroll-none p-1.5"
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
                    className={`flex h-9 w-full items-center gap-2.5 rounded-lg px-2.5 text-left text-[13px] ${
                      highlighted
                        ? "bg-selection text-content"
                        : "text-content/75 hover:bg-content/5 hover:text-content"
                    }`}
                  >
                    <span className="grid size-4 shrink-0 place-items-center">
                      {isSelected ? (
                        <Check className="size-3.5" strokeWidth={2} />
                      ) : null}
                    </span>
                    <span className="min-w-0 flex-1 truncate">
                      {option.label}
                    </span>
                  </button>
                );
              })
            ) : (
              <p className="px-2.5 py-5 text-center text-[12px] text-content/45">
                {emptyLabel}
              </p>
            )}
          </div>
        </Popover>
      ) : null}
    </div>
  );
}

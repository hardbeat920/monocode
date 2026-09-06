import { Check, ChevronDown, Search } from "./icons";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import { HarnessIcon } from "./HarnessIcon";
import { Popover } from "./Popover";
import {
  isFreeOpenCodeModel,
  loadOpenCodeFreeOnly,
  saveOpenCodeFreeOnly,
  sortModelsNewestFirst,
  type AgentModel,
} from "../lib/models";
import { HARNESS_TITLE, type HarnessId } from "../lib/session";

type Props = {
  label: string;
  harness: HarnessId;
  value: string;
  models: AgentModel[];
  onChange: (value: string) => void;
};

const MENU_WIDTH = 300;
const MENU_MIN_HEIGHT = 180;
const MENU_MAX_HEIGHT = 340;

export function ProviderModelSelect({
  label,
  harness,
  value,
  models,
  onChange,
}: Props) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [freeOnly, setFreeOnly] = useState(
    () => harness === "opencode" && loadOpenCodeFreeOnly(),
  );
  const root = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const current = models.find((item) => item.id === value) ?? models[0];

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return models.filter((item) => {
      if (harness === "opencode" && freeOnly && !isFreeOpenCodeModel(item)) {
        return false;
      }
      if (!needle) return true;
      const hay = `${item.name} ${item.nativeId ?? ""} ${item.id}`.toLowerCase();
      return hay.includes(needle);
    });
  }, [models, query, harness, freeOnly]);

  const dismiss = () => {
    setOpen(false);
    setQuery("");
  };

  useEffect(() => {
    if (!open) return;
    setQuery("");
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const index = visible.findIndex((item) => item.id === value);
    setActive(index >= 0 ? index : 0);
  }, [open, visible, value]);

  useEffect(() => {
    if (open) search.current?.focus();
  }, [open]);

  useEffect(() => {
    setActive((index) =>
      visible.length === 0 ? 0 : Math.min(index, visible.length - 1),
    );
  }, [visible.length]);

  const pick = (item: AgentModel) => {
    onChange(item.id);
    dismiss();
  };

  const toggleFreeOnly = () => {
    const next = !freeOnly;
    setFreeOnly(next);
    saveOpenCodeFreeOnly(next);
    if (!next) return;
    const currentModel = models.find((item) => item.id === value);
    if (currentModel && isFreeOpenCodeModel(currentModel)) return;
    const newestFree = sortModelsNewestFirst(
      models.filter(isFreeOpenCodeModel),
    )[0];
    if (newestFree) onChange(newestFree.id);
  };

  const onSearchKey = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActive((index) => Math.min(visible.length - 1, index + 1));
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      setActive((index) => Math.max(0, index - 1));
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      const item = visible[active];
      if (item) pick(item);
    }
  };

  if (!current) return null;

  return (
    <div ref={root} className="relative">
      <button
        type="button"
        title={`${HARNESS_TITLE[harness]} · ${current.name}`}
        aria-label={label}
        aria-expanded={open}
        aria-haspopup="listbox"
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => {
          if (open) {
            dismiss();
            return;
          }
          setOpen(true);
        }}
        className={`flex h-6.5 max-w-52 items-center gap-1 rounded-md px-1.5 ${
          open
            ? "bg-content/10 text-content"
            : "bg-content/10 text-content hover:bg-content/15"
        }`}
      >
        <HarnessIcon harness={harness} className="size-4 shrink-0" />
        <span className="min-w-0 truncate text-[11px]">{current.name}</span>
        <ChevronDown
          className={`size-3 shrink-0 text-content/50 ${open ? "rotate-180" : ""}`}
          strokeWidth={1.75}
        />
      </button>
      {open ? (
        <Popover
          anchor={root}
          side="bottom"
          align="end"
          width={MENU_WIDTH}
          minHeight={MENU_MIN_HEIGHT}
          maxHeight={MENU_MAX_HEIGHT}
          onDismiss={dismiss}
          role="dialog"
          aria-label={label}
          data-provider-model-picker
          className="flex flex-col overflow-hidden"
        >
          <label className="flex shrink-0 items-center gap-2 border-b border-content/10 px-2 py-2.5 text-content/50">
            <Search className="size-3.5 shrink-0" strokeWidth={1.75} />
            <input
              ref={search}
              type="text"
              value={query}
              placeholder="Search models..."
              aria-label="Search models"
              spellCheck={false}
              autoComplete="off"
              autoCorrect="off"
              autoCapitalize="off"
              className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/40"
              onChange={(event) => {
                setQuery(event.target.value);
                setActive(0);
              }}
              onKeyDown={onSearchKey}
            />
            {harness === "opencode" ? (
              <button
                type="button"
                aria-pressed={freeOnly}
                aria-label="Show free OpenCode models"
                title="Show free OpenCode models"
                onMouseDown={(event) => event.preventDefault()}
                onClick={toggleFreeOnly}
                className={`shrink-0 rounded-md px-1.5 py-0.5 text-[11px] font-medium ${
                  freeOnly
                    ? "bg-content/15 text-content"
                    : "text-content/50 hover:bg-content/10 hover:text-content"
                }`}
              >
                Free
              </button>
            ) : null}
          </label>
          <ModelList
            models={visible}
            active={active}
            currentId={current.id}
            emptyLabel={
              query.trim()
                ? "No matching models"
                : harness === "opencode" && freeOnly
                  ? "No free models available"
                  : "No models available"
            }
            onActive={setActive}
            onPick={pick}
          />
        </Popover>
      ) : null}
    </div>
  );
}

function ModelList({
  models,
  active,
  currentId,
  emptyLabel,
  onActive,
  onPick,
}: {
  models: AgentModel[];
  active: number;
  currentId: string;
  emptyLabel: string;
  onActive: (index: number) => void;
  onPick: (model: AgentModel) => void;
}) {
  const listRef = useRef<HTMLDivElement>(null);
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const activeRef = useRef<HTMLButtonElement>(null);

  const setListRef = (el: HTMLDivElement | null) => {
    listRef.current = el;
    lockOverscroll(el);
  };

  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [active]);

  useEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const onWheel = (event: WheelEvent) => {
      event.stopPropagation();
      if (el.scrollHeight <= el.clientHeight + 1) return;
      el.scrollTop += event.deltaY;
      event.preventDefault();
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [models.length]);

  if (models.length === 0) {
    return (
      <div className="px-3 py-4 text-[12px] text-content/50">{emptyLabel}</div>
    );
  }

  return (
    <div
      ref={setListRef}
      role="listbox"
      aria-label="Models"
      className="min-h-0 flex-1 overflow-y-auto overscroll-none px-1.5 py-1.5"
    >
      {models.map((item, index) => {
        const selected = item.id === currentId;
        const highlighted = index === active;
        return (
          <button
            key={item.id}
            ref={highlighted ? activeRef : undefined}
            type="button"
            role="option"
            aria-selected={selected}
            onMouseDown={(event) => event.preventDefault()}
            onMouseEnter={() => onActive(index)}
            onClick={() => onPick(item)}
            className={`flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left ${
              highlighted || selected
                ? "bg-content/10 text-content"
                : "text-content hover:bg-content/5"
            }`}
          >
            <span className="min-w-0 flex-1 truncate text-[13px] font-medium leading-5">
              {item.name}
            </span>
            {selected ? (
              <Check className="size-3.5 shrink-0" strokeWidth={1.75} />
            ) : null}
          </button>
        );
      })}
    </div>
  );
}

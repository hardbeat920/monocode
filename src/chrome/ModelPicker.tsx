import { ChevronDown, Search, Star } from "lucide-react";
import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import {
  coerceModelPickerTab,
  findModel,
  getModelSnapshot,
  getPickerVisibilitySnapshot,
  loadFavoriteModels,
  loadModelPickerTab,
  modelsFor,
  resolveModel,
  saveFavoriteModels,
  saveModelPickerTab,
  showProviderInModelPicker,
  stepModelPickerTab,
  subscribeModels,
  subscribePickerVisibility,
  type AgentModel,
  type ModelPickerTab,
} from "../lib/models";
import {
  harnessUnavailableHint,
  hasProbedHarnessAvailability,
  isHarnessAvailable,
  probeHarnessAvailability,
  subscribeHarnessAvailability,
  getHarnessAvailabilitySnapshot,
} from "../lib/harness/availability";
import { refreshHarnessCatalogs } from "../lib/harness/registry";
import {
  HARNESSES,
  HARNESS_LABEL,
  HARNESS_TITLE,
  type HarnessId,
} from "../lib/session";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import { HarnessIcon } from "./HarnessIcon";
import { MOD } from "../lib/platform";

type Props = {
  harness: HarnessId;
  model: string;
  hotkeys?: boolean;
  onChange: (harness: HarnessId, model: string) => void;
  onClose?: () => void;
};

const MENU_WIDTH = 300;

function menuStyle(anchor: DOMRect): CSSProperties {
  const width = Math.min(MENU_WIDTH, window.innerWidth - 16);
  const left = Math.min(
    Math.max(8, anchor.left),
    window.innerWidth - width - 8,
  );
  return {
    position: "fixed",
    left,
    bottom: window.innerHeight - anchor.top + 6,
    width,
    height: Math.max(180, Math.min(340, anchor.top - 12)),
    zIndex: 80,
  };
}

export function ModelPicker({
  harness,
  model,
  hotkeys = false,
  onChange,
  onClose,
}: Props) {
  const catalogVersion = useSyncExternalStore(
    subscribeModels,
    getModelSnapshot,
    getModelSnapshot,
  );
  const availabilityVersion = useSyncExternalStore(
    subscribeHarnessAvailability,
    getHarnessAvailabilitySnapshot,
    getHarnessAvailabilitySnapshot,
  );
  const visibilityVersion = useSyncExternalStore(
    subscribePickerVisibility,
    getPickerVisibilitySnapshot,
    getPickerVisibilitySnapshot,
  );
  const [open, setOpen] = useState(false);
  const [tab, setTab] = useState<ModelPickerTab>(() => loadModelPickerTab());
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [favorites, setFavorites] = useState(loadFavoriteModels);
  const [menu, setMenu] = useState<CSSProperties>();
  const root = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const current = resolveModel(harness, model);
  const tabRef = useRef(tab);
  const openRef = useRef(open);
  const lastHotkey = useRef(0);

  const shownInPicker = (id: HarnessId) =>
    showProviderInModelPicker(
      id,
      isHarnessAvailable(id),
      hasProbedHarnessAvailability(),
    );
  const pickerHarnesses = HARNESSES.filter(shownInPicker);
  const visibleTab = coerceModelPickerTab(tab, shownInPicker);
  if (visibleTab !== tab) {
    setTab(visibleTab);
  }
  tabRef.current = visibleTab;
  openRef.current = open;

  const dismiss = (restore: boolean) => {
    setOpen(false);
    if (restore) onCloseRef.current?.();
  };

  const openPicker = () => {
    const rect = root.current?.getBoundingClientRect();
    if (rect) setMenu(menuStyle(rect));
    setOpen(true);
  };

  const togglePicker = () => {
    if (openRef.current) dismiss(true);
    else openPicker();
  };

  const toggleFromHotkey = () => {
    const now = performance.now();
    if (now - lastHotkey.current < 80) return;
    lastHotkey.current = now;
    togglePicker();
  };

  const selectTab = (next: ModelPickerTab) => {
    setTab(next);
    saveModelPickerTab(next);
  };

  useEffect(() => {
    if (!open) return;
    void probeHarnessAvailability();
    setTab(coerceModelPickerTab(loadModelPickerTab(), shownInPicker));
    setQuery("");
  }, [open]);

  useEffect(() => {
    if (!open || visibleTab === "favorites") return;
    void refreshHarnessCatalogs([visibleTab]);
  }, [open, visibleTab]);

  useLayoutEffect(() => {
    if (!open || !root.current) return;
    const place = () => {
      const rect = root.current?.getBoundingClientRect();
      if (rect) setMenu(menuStyle(rect));
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: PointerEvent) => {
      const target = e.target as Node;
      if (root.current?.contains(target) || menuRef.current?.contains(target)) {
        return;
      }
      dismiss(false);
    };
    window.addEventListener("pointerdown", onPointerDown);
    return () => window.removeEventListener("pointerdown", onPointerDown);
  }, [open]);

  useEffect(() => {
    const inBlockingUi = (target: EventTarget | null) => {
      if (!(target instanceof Element)) return false;
      if (target.closest(".monocode-terminal")) return true;
      return Boolean(
        target.closest(
          "[data-file-picker], [data-branch-picker], [data-skill-picker], [data-mention-picker], [data-access-picker], [data-model-settings]",
        ),
      );
    };

    const onKey = (e: KeyboardEvent) => {
      if (e.isComposing) return;
      const mod = e.metaKey || e.ctrlKey;
      if (
        hotkeys &&
        mod &&
        !e.altKey &&
        !e.shiftKey &&
        (e.key === "." || e.code === "Period")
      ) {
        if (!openRef.current && inBlockingUi(e.target)) return;
        e.preventDefault();
        e.stopPropagation();
        toggleFromHotkey();
        return;
      }
      if (!openRef.current) return;
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        dismiss(true);
        return;
      }
      if (mod || e.altKey || e.shiftKey) return;
      if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
      if (inBlockingUi(e.target)) return;
      e.preventDefault();
      e.stopPropagation();
      selectTab(
        stepModelPickerTab(
          tabRef.current,
          e.key === "ArrowLeft" ? -1 : 1,
          shownInPicker,
        ),
      );
    };

    const onMenu = () => {
      if (!hotkeys) return;
      if (inBlockingUi(document.activeElement)) return;
      toggleFromHotkey();
    };

    window.addEventListener("keydown", onKey, true);
    window.addEventListener("open_model_picker", onMenu);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("open_model_picker", onMenu);
    };
  }, [hotkeys]);

  useEffect(() => {
    if (open) search.current?.focus();
  }, [open, menu]);

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const pool =
      visibleTab === "favorites"
        ? favorites
            .map((id) => findModel(id))
            .filter(
              (item): item is AgentModel =>
                item != null && shownInPicker(item.harness),
            )
        : modelsFor(visibleTab);
    if (!needle) return pool;
    return pool.filter((item) => {
      const hay =
        `${item.name} ${HARNESS_TITLE[item.harness]} ${HARNESS_LABEL[item.harness]}`.toLowerCase();
      return hay.includes(needle);
    });
    // Catalog, install probes, and picker-visibility all feed this list:
    // catalogs land after mount, and hiding a provider must drop its favorites.
  }, [
    visibleTab,
    query,
    favorites,
    catalogVersion,
    availabilityVersion,
    visibilityVersion,
  ]);

  useEffect(() => {
    if (!open) return;
    const index = visible.findIndex((item) => item.id === current.id);
    setActive(index >= 0 ? index : 0);
  }, [open, visibleTab, query, current.id]);

  useEffect(() => {
    setActive((i) =>
      visible.length === 0 ? 0 : Math.min(i, visible.length - 1),
    );
  }, [visible.length]);

  const pick = (item: AgentModel) => {
    if (!isHarnessAvailable(item.harness)) return;
    onChange(item.harness, item.id);
    dismiss(true);
  };

  const toggleFavorite = (id: string) => {
    setFavorites((prev) => {
      const next = prev.includes(id)
        ? prev.filter((item) => item !== id)
        : [...prev, id];
      saveFavoriteModels(next);
      return next;
    });
  };

  const onSearchKey = (e: ReactKeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((i) => Math.min(visible.length - 1, i + 1));
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((i) => Math.max(0, i - 1));
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      const item = visible[active];
      if (item && isHarnessAvailable(item.harness)) pick(item);
      return;
    }
    const mod = e.metaKey || e.ctrlKey;
    if (mod && !e.altKey && !e.shiftKey && e.key >= "1" && e.key <= "9") {
      e.preventDefault();
      const item = visible[Number(e.key) - 1];
      if (item && isHarnessAvailable(item.harness)) pick(item);
    }
  };

  return (
    <div ref={root} className="relative">
      <button
        type="button"
        title={`${HARNESS_TITLE[current.harness]} · ${current.name} (${MOD}.)`}
        aria-label={`${HARNESS_TITLE[current.harness]} ${current.name}`}
        aria-keyshortcuts={`${MOD}.`}
        aria-expanded={open}
        aria-haspopup="dialog"
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => {
          if (open) {
            dismiss(true);
            return;
          }
          openPicker();
        }}
        className={`flex h-6.5 max-w-52 items-center gap-1 rounded-md px-1.5 ${
          open
            ? "bg-content/10 text-content"
            : "bg-content/10 text-content hover:bg-content/15"
        }`}
      >
        <HarnessIcon harness={current.harness} className="size-4 shrink-0" />
        <span className="min-w-0 truncate text-[11px]">{current.name}</span>
        <ChevronDown
          className={`size-3 shrink-0 text-content/50 ${open ? "rotate-180" : ""}`}
          strokeWidth={1.75}
        />
      </button>
      {/* Portalled: the composer box is a `z-10` stacking context, so an inline
          fixed menu loses to anything painted above it — in a split pane the
          arcade backdrop swallowed the list (#29). */}
      {open && menu
        ? createPortal(
            <div
              ref={menuRef}
              role="dialog"
              aria-label="Model picker"
              data-model-picker
              style={menu}
              className="flex flex-col overflow-hidden rounded-lg border border-content/10 bg-content/10 shadow-xl backdrop-blur-xl"
            >
              <nav
                role="tablist"
                aria-label="Providers"
                aria-keyshortcuts="ArrowLeft ArrowRight"
                aria-orientation="horizontal"
                className="flex w-full shrink-0 items-stretch border-b border-content/10"
              >
                <ProviderTabButton
                  title="Favorites"
                  selected={visibleTab === "favorites"}
                  onSelect={() => selectTab("favorites")}
                >
                  <Star
                    className="size-4"
                    strokeWidth={1.75}
                    fill={visibleTab === "favorites" ? "currentColor" : "none"}
                  />
                </ProviderTabButton>
                {pickerHarnesses.map((id) => (
                  <ProviderTabButton
                    key={id}
                    title={HARNESS_TITLE[id]}
                    selected={visibleTab === id}
                    onSelect={() => selectTab(id)}
                  >
                    <HarnessIcon harness={id} className="size-4" />
                  </ProviderTabButton>
                ))}
              </nav>

              <div className="flex min-h-0 min-w-0 flex-1 flex-col">
                <div className="pb-1.5">
                  <label className="flex items-center gap-2 border-b border-content/10 px-2 py-2.5 text-content/50">
                    <Search className="size-3.5 shrink-0" strokeWidth={1.75} />
                    <input
                      ref={search}
                      type="text"
                      value={query}
                      placeholder="Search models..."
                      aria-label="Search models"
                      className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/40"
                      onChange={(e) => setQuery(e.target.value)}
                      onKeyDown={onSearchKey}
                    />
                  </label>
                </div>
                <ModelList
                  models={visible}
                  active={active}
                  currentId={current.id}
                  favorites={favorites}
                  emptyLabel={
                    visibleTab === "favorites" && !query.trim()
                      ? "No favorite models"
                      : visibleTab !== "favorites" &&
                          !isHarnessAvailable(visibleTab)
                        ? harnessUnavailableHint(visibleTab)
                        : visibleTab === "codex" && !query.trim()
                          ? "Loading Codex models…"
                          : "No matching models"
                  }
                  onActive={setActive}
                  onPick={pick}
                  onToggleFavorite={toggleFavorite}
                />
              </div>
            </div>,
            document.body,
          )
        : null}
    </div>
  );
}

function ProviderTabButton({
  title,
  selected,
  disabled = false,
  onSelect,
  children,
}: {
  title: string;
  selected: boolean;
  disabled?: boolean;
  onSelect: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      role="tab"
      title={title}
      aria-label={title}
      aria-selected={selected}
      aria-disabled={disabled}
      disabled={disabled}
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => {
        if (disabled) return;
        onSelect();
      }}
      className={`relative flex min-w-0 flex-1 items-center justify-center gap-1 px-2 py-3 text-[11px] leading-4 ${
        disabled
          ? "cursor-not-allowed text-content/25"
          : selected
            ? "bg-content/10 text-content"
            : "text-content/50 hover:bg-content/5 hover:text-content"
      }`}
    >
      <span className="shrink-0">{children}</span>
      {selected && !disabled ? (
        <span className="absolute inset-x-0 bottom-0 h-0.5 rounded-full bg-content" />
      ) : null}
    </button>
  );
}

function ModelList({
  models,
  active,
  currentId,
  favorites,
  emptyLabel,
  onActive,
  onPick,
  onToggleFavorite,
}: {
  models: AgentModel[];
  active: number;
  currentId: string;
  favorites: string[];
  emptyLabel: string;
  onActive: (index: number) => void;
  onPick: (model: AgentModel) => void;
  onToggleFavorite: (id: string) => void;
}) {
  const listRef = useRef<HTMLDivElement>(null);
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const activeRef = useRef<HTMLDivElement>(null);

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
    const onWheel = (e: WheelEvent) => {
      e.stopPropagation();
      if (el.scrollHeight <= el.clientHeight + 1) return;
      el.scrollTop += e.deltaY;
      e.preventDefault();
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
      className="min-h-0 flex-1 overflow-y-auto overscroll-none px-1.5 pb-1.5"
    >
      {models.map((item, index) => {
        const selected = item.id === currentId;
        const highlighted = index === active;
        const favorited = favorites.includes(item.id);
        const disabled = !isHarnessAvailable(item.harness);
        const shortcut = index < 9 && !disabled ? `${MOD}${index + 1}` : null;
        return (
          <div
            key={item.id}
            ref={highlighted ? activeRef : undefined}
            onMouseEnter={() => onActive(index)}
            className={`flex w-full items-center gap-1 rounded-lg px-1 ${
              disabled
                ? ""
                : highlighted || selected
                  ? "bg-content/10"
                  : "hover:bg-content/5"
            }`}
          >
            <button
              type="button"
              role="option"
              aria-selected={selected}
              aria-disabled={disabled}
              disabled={disabled}
              title={
                disabled ? harnessUnavailableHint(item.harness) : undefined
              }
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                if (disabled) return;
                onPick(item);
              }}
              className={`flex min-w-0 flex-1 items-center gap-2 px-1.5 py-2 text-left ${
                disabled ? "cursor-not-allowed text-content/35" : "text-content"
              }`}
            >
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[13px] font-medium leading-5">
                  {item.name}
                </span>
                <span className="mt-0.5 flex items-center gap-1 text-[11px] leading-4 text-content/50">
                  <HarnessIcon
                    harness={item.harness}
                    className="size-3 shrink-0 opacity-80"
                  />
                  <span className="truncate">
                    {HARNESS_TITLE[item.harness]} ·{" "}
                    {HARNESS_LABEL[item.harness]}
                  </span>
                </span>
              </span>
              {shortcut ? (
                <span className="shrink-0 rounded-md bg-content/10 px-1.5 py-0.5 font-mono text-[10px] text-content/50">
                  {shortcut}
                </span>
              ) : null}
            </button>
            <button
              type="button"
              title={favorited ? "Remove from favorites" : "Add to favorites"}
              aria-label={
                favorited ? "Remove from favorites" : "Add to favorites"
              }
              onMouseDown={(e) => e.preventDefault()}
              onClick={(e) => {
                e.stopPropagation();
                onToggleFavorite(item.id);
              }}
              className={`grid size-6 shrink-0 place-items-center rounded-md ${
                favorited
                  ? "text-content"
                  : "text-content/30 hover:text-content/70"
              }`}
            >
              <Star
                className="size-3.5"
                strokeWidth={1.75}
                fill={favorited ? "currentColor" : "none"}
              />
            </button>
          </div>
        );
      })}
    </div>
  );
}

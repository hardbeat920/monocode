import { Check, ChevronDown, ChevronRight, Search, Star } from "./icons";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import {
  coerceModelPickerTab,
  findModel,
  getModelSnapshot,
  getPickerVisibilitySnapshot,
  loadFavoriteModels,
  modelsFor,
  resolveModel,
  saveFavoriteModels,
  showProviderInModelPicker,
  subscribeModels,
  subscribePickerVisibility,
  type AgentModel,
  type ModelPickerTab,
  type ModelSetting,
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
import { HARNESSES, HARNESS_TITLE, type HarnessId } from "../lib/session";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import { LAYER } from "../lib/layers";
import { HarnessIcon } from "./HarnessIcon";
import { Popover } from "./Popover";
import { MOD } from "../lib/platform";

type Props = {
  harness: HarnessId;
  model: string;
  values: Record<string, string>;
  hotkeys?: boolean;
  onChange: (harness: HarnessId, model: string) => void;
  onSettingsChange: (settings: Record<string, string>) => void;
  onClose?: () => void;
};

type MenuEntry = { kind: "setting"; setting: ModelSetting } | { kind: "model" };

type Submenu = { kind: "setting"; setting: ModelSetting } | { kind: "models" };

const MENU_WIDTH = 250;
const MODEL_MENU_WIDTH = 310;
const SETTING_MENU_WIDTH = 210;
const SUBMENU_OVERLAP = -4;
const SELF = "[data-model-picker]";

const PROVIDER_TAB_SIZE = 32;
const PROVIDER_TAB_GAP = 4;
const PROVIDER_RAIL_PADDING = 12;
const MODEL_MENU_HEIGHT =
  (HARNESSES.length + 1) * PROVIDER_TAB_SIZE +
  HARNESSES.length * PROVIDER_TAB_GAP +
  PROVIDER_RAIL_PADDING;
const MODEL_MENU_FRAME_HEIGHT = MODEL_MENU_HEIGHT + 2;

const SETTING_ORDER = [
  "fast",
  "effort",
  "reasoning",
  "thinking",
  "variant",
  "agent",
  "context",
];

function pickerSettings(model: AgentModel): ModelSetting[] {
  return [...(model.settings ?? [])]
    .filter(
      (setting) => !(model.harness === "opencode" && setting.id === "agent"),
    )
    .sort((a, b) => {
      const ai = SETTING_ORDER.indexOf(a.id);
      const bi = SETTING_ORDER.indexOf(b.id);
      return (ai < 0 ? 99 : ai) - (bi < 0 ? 99 : bi);
    });
}

function settingLabel(setting: ModelSetting): string {
  return setting.id === "effort" || setting.id === "reasoning"
    ? "Effort"
    : setting.label;
}

function settingValue(
  setting: ModelSetting,
  values: Record<string, string>,
): string {
  return values[setting.id] ?? setting.value;
}

function settingValueLabel(
  setting: ModelSetting,
  values: Record<string, string>,
): string {
  const value = settingValue(setting, values);
  return (
    setting.options.find((option) => option.value === value)?.label ?? value
  );
}

export function ModelPicker({
  harness,
  model,
  values,
  hotkeys = false,
  onChange,
  onSettingsChange,
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
  const [tab, setTab] = useState<ModelPickerTab>(harness);
  const [active, setActive] = useState(0);
  const [activeModel, setActiveModel] = useState(0);
  const [activeSetting, setActiveSetting] = useState(0);
  const [submenu, setSubmenu] = useState<Submenu | null>(null);
  const [activeRow, setActiveRow] = useState<HTMLButtonElement | null>(null);
  const [query, setQuery] = useState("");
  const [favorites, setFavorites] = useState(loadFavoriteModels);
  const button = useRef<HTMLButtonElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const onCloseRef = useRef(onClose);
  const openRef = useRef(open);
  const lastHotkey = useRef(0);
  onCloseRef.current = onClose;
  openRef.current = open;

  const current = resolveModel(harness, model);
  const settings = useMemo(() => {
    void catalogVersion;
    return pickerSettings(current);
  }, [catalogVersion, current]);
  const entries = useMemo<MenuEntry[]>(
    () => [
      ...settings.map((setting) => ({
        kind: "setting" as const,
        setting,
      })),
      { kind: "model" as const },
    ],
    [settings],
  );

  const triggerLabel = current.name;

  const pickerHarnesses = useMemo(() => {
    void availabilityVersion;
    void visibilityVersion;
    return HARNESSES.filter((id) =>
      showProviderInModelPicker(
        id,
        isHarnessAvailable(id),
        hasProbedHarnessAvailability(),
      ),
    );
  }, [availabilityVersion, visibilityVersion]);
  const providerKey = pickerHarnesses.join(",");
  const visibleTab = coerceModelPickerTab(tab, (id) =>
    pickerHarnesses.includes(id),
  );

  const visibleModels = useMemo(() => {
    void catalogVersion;
    const needle = query.trim().toLowerCase();
    const pool =
      visibleTab === "favorites"
        ? favorites
            .map((id) => findModel(id))
            .filter(
              (item): item is AgentModel =>
                item != null && pickerHarnesses.includes(item.harness),
            )
        : modelsFor(visibleTab);
    if (!needle) return pool;
    return pool.filter((item) =>
      `${item.name} ${HARNESS_TITLE[item.harness]}`
        .toLowerCase()
        .includes(needle),
    );
  }, [catalogVersion, favorites, providerKey, query, visibleTab]);

  const dismiss = (restore: boolean) => {
    setOpen(false);
    setSubmenu(null);
    if (restore) onCloseRef.current?.();
  };

  const togglePicker = () => {
    if (openRef.current) dismiss(true);
    else setOpen(true);
  };

  const toggleFromHotkey = () => {
    const now = performance.now();
    if (now - lastHotkey.current < 80) return;
    lastHotkey.current = now;
    togglePicker();
  };

  useEffect(() => {
    if (!open) return;
    void probeHarnessAvailability();
    void refreshHarnessCatalogs([current.harness]);
    setTab(
      coerceModelPickerTab(current.harness, (id) =>
        pickerHarnesses.includes(id),
      ),
    );
    setActive(0);
    setSubmenu(null);
    setQuery("");
    setFavorites(loadFavoriteModels());
  }, [open, current.harness]);

  useEffect(() => {
    if (visibleTab === tab) return;
    setTab(visibleTab);
  }, [tab, visibleTab]);

  useEffect(() => {
    if (!open || submenu?.kind !== "models" || visibleTab === "favorites") {
      return;
    }
    void refreshHarnessCatalogs([visibleTab]);
  }, [open, submenu?.kind, visibleTab]);

  useEffect(() => {
    if (!open) return;
    setActive((index) => Math.min(index, Math.max(0, entries.length - 1)));
  }, [entries.length, open]);

  useEffect(() => {
    if (!open || submenu?.kind !== "models") return;
    const index = visibleModels.findIndex((item) => item.id === current.id);
    setActiveModel(index >= 0 ? index : 0);
  }, [open, submenu?.kind, query, visibleModels, current.id]);

  useEffect(() => {
    if (submenu?.kind !== "setting") return;
    const value = settingValue(submenu.setting, values);
    const index = submenu.setting.options.findIndex(
      (option) => option.value === value,
    );
    setActiveSetting(index >= 0 ? index : 0);
  }, [submenu, values]);

  useEffect(() => {
    const inBlockingUi = (target: EventTarget | null) => {
      if (!(target instanceof Element)) return false;
      if (target.closest(".monocode-terminal")) return true;
      return Boolean(
        target.closest(
          "[data-file-picker], [data-branch-picker], [data-skill-picker], [data-mention-picker], [data-access-picker]",
        ),
      );
    };

    const onKey = (event: KeyboardEvent) => {
      if (event.isComposing) return;
      const mod = event.metaKey || event.ctrlKey;
      if (
        hotkeys &&
        mod &&
        !event.altKey &&
        !event.shiftKey &&
        (event.key === "." || event.code === "Period")
      ) {
        if (!openRef.current && inBlockingUi(event.target)) return;
        event.preventDefault();
        event.stopPropagation();
        toggleFromHotkey();
        return;
      }
      if (!openRef.current || event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      dismiss(true);
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

  const setSetting = (setting: ModelSetting, value: string) => {
    onSettingsChange({ ...values, [setting.id]: value });
  };

  const pickModel = (item: AgentModel) => {
    if (!isHarnessAvailable(item.harness)) return;
    onChange(item.harness, item.id);
    dismiss(true);
  };

  const pickSetting = (setting: ModelSetting, value: string) => {
    setSetting(setting, value);
    dismiss(true);
  };

  const toggleFavorite = (id: string) => {
    setFavorites((previous) => {
      const next = previous.includes(id)
        ? previous.filter((item) => item !== id)
        : [...previous, id];
      saveFavoriteModels(next);
      return next;
    });
  };

  const selectTab = (next: ModelPickerTab) => {
    setTab(next);
    setQuery("");
    setActiveModel(0);
  };

  const showEntrySubmenu = (entry: MenuEntry) => {
    if (entry.kind === "model") {
      setSubmenu({ kind: "models" });
      return;
    }
    if (entry.setting.kind === "select") {
      setSubmenu({ kind: "setting", setting: entry.setting });
      return;
    }
    setSubmenu(null);
  };

  const moveEntry = (direction: 1 | -1) => {
    setSubmenu(null);
    setActive((index) => (index + direction + entries.length) % entries.length);
  };

  const onMenuKey = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.target instanceof HTMLInputElement) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      if (submenu?.kind === "models") {
        setActiveModel((index) =>
          Math.min(visibleModels.length - 1, index + 1),
        );
      } else if (submenu?.kind === "setting") {
        setActiveSetting((index) =>
          Math.min(submenu.setting.options.length - 1, index + 1),
        );
      } else {
        moveEntry(1);
      }
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      if (submenu?.kind === "models") {
        setActiveModel((index) => Math.max(0, index - 1));
      } else if (submenu?.kind === "setting") {
        setActiveSetting((index) => Math.max(0, index - 1));
      } else {
        moveEntry(-1);
      }
      return;
    }
    if (event.key === "ArrowRight") {
      event.preventDefault();
      const entry = entries[active];
      if (entry) showEntrySubmenu(entry);
      return;
    }
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      setSubmenu(null);
      return;
    }
    if (event.key !== "Enter") return;
    event.preventDefault();
    if (submenu?.kind === "models") {
      const item = visibleModels[activeModel];
      if (item) pickModel(item);
      return;
    }
    if (submenu?.kind === "setting") {
      const option = submenu.setting.options[activeSetting];
      if (option) pickSetting(submenu.setting, option.value);
      return;
    }
    const entry = entries[active];
    if (!entry) return;
    if (entry.kind === "model" || entry.setting.kind === "select") {
      showEntrySubmenu(entry);
      return;
    }
    const value = settingValue(entry.setting, values);
    setSetting(entry.setting, value === "true" ? "false" : "true");
  };

  const showSubmenu =
    open &&
    submenu != null &&
    activeRow != null &&
    activeRow.dataset.modelControlIndex === String(active);

  return (
    <>
      <button
        ref={button}
        type="button"
        title={`${HARNESS_TITLE[current.harness]} · ${current.name} (${MOD}.)`}
        aria-label={`${HARNESS_TITLE[current.harness]} ${current.name}`}
        aria-keyshortcuts={`${MOD}.`}
        aria-expanded={open}
        aria-haspopup="menu"
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => togglePicker()}
        className={`flex h-6.5 max-w-40 items-center gap-1 rounded-md px-1.5 ${
          open
            ? "bg-content/10 text-content"
            : "bg-content/10 text-content hover:bg-content/15"
        }`}
      >
        <HarnessIcon harness={current.harness} className="size-4 shrink-0" />
        <span className="min-w-0 truncate text-[11px]">{triggerLabel}</span>
        <ChevronDown
          className={`size-3 shrink-0 text-content/50 ${open ? "rotate-180" : ""}`}
          strokeWidth={1.75}
        />
      </button>

      {open ? (
        <>
          <Popover
            anchor={button}
            side="top"
            width={MENU_WIDTH}
            autoFocus
            dismissOnEscape={false}
            ignore={SELF}
            onDismiss={() => dismiss(false)}
            role="menu"
            aria-label="Model and effort"
            tabIndex={-1}
            onKeyDown={onMenuKey}
            data-model-picker
            className="p-1 font-sans"
          >
            {entries.map((entry, index) => {
              const highlighted = index === active;
              if (entry.kind === "model") {
                return (
                  <button
                    key="model"
                    ref={highlighted ? setActiveRow : undefined}
                    data-model-control-index={index}
                    type="button"
                    role="menuitem"
                    aria-haspopup="menu"
                    aria-expanded={highlighted && showSubmenu}
                    onMouseDown={(event) => event.preventDefault()}
                    onMouseEnter={() => {
                      setActive(index);
                      showEntrySubmenu(entry);
                    }}
                    onClick={() => showEntrySubmenu(entry)}
                    className={`flex h-9 w-full items-center gap-2 rounded-lg px-2 text-left text-[13px] ${
                      highlighted
                        ? "bg-content/10 text-content"
                        : "text-content hover:bg-content/5"
                    }`}
                  >
                    <span className="min-w-0 flex-1">Model</span>
                    <span className="flex min-w-0 max-w-36 items-center gap-1 text-content/55">
                      <HarnessIcon
                        harness={current.harness}
                        className="size-3.5 shrink-0"
                      />
                      <span className="min-w-0 truncate">{current.name}</span>
                    </span>
                    <ChevronRight
                      className="size-3.5 shrink-0 text-content/45"
                      strokeWidth={1.75}
                    />
                  </button>
                );
              }

              const setting = entry.setting;
              const value = settingValue(setting, values);
              const isToggle = setting.kind === "toggle";
              return (
                <button
                  key={setting.id}
                  ref={highlighted ? setActiveRow : undefined}
                  data-model-control-index={index}
                  type="button"
                  role={isToggle ? "menuitemcheckbox" : "menuitem"}
                  aria-checked={isToggle ? value === "true" : undefined}
                  aria-haspopup={isToggle ? undefined : "menu"}
                  aria-expanded={
                    !isToggle && highlighted ? showSubmenu : undefined
                  }
                  title={setting.description}
                  onMouseDown={(event) => event.preventDefault()}
                  onMouseEnter={() => {
                    setActive(index);
                    showEntrySubmenu(entry);
                  }}
                  onClick={() => {
                    if (isToggle) {
                      setSetting(setting, value === "true" ? "false" : "true");
                    } else {
                      showEntrySubmenu(entry);
                    }
                  }}
                  className={`flex h-9 w-full items-center gap-2 rounded-lg px-2 text-left text-[13px] ${
                    highlighted
                      ? "bg-content/10 text-content"
                      : "text-content hover:bg-content/5"
                  }`}
                >
                  <span className="min-w-0 flex-1">
                    {settingLabel(setting)}
                  </span>
                  {isToggle ? (
                    <span
                      aria-hidden="true"
                      className={`relative h-5 w-9 shrink-0 rounded-full transition-colors ${
                        value === "true" ? "bg-content/35" : "bg-content/15"
                      }`}
                    >
                      <span
                        className={`absolute top-0.5 size-4 rounded-full bg-content shadow-sm transition-transform ${
                          value === "true"
                            ? "translate-x-4.5"
                            : "translate-x-0.5"
                        }`}
                      />
                    </span>
                  ) : (
                    <>
                      <span className="min-w-0 max-w-28 truncate text-content/55">
                        {settingValueLabel(setting, values)}
                      </span>
                      <ChevronRight
                        className="size-3.5 shrink-0 text-content/45"
                        strokeWidth={1.75}
                      />
                    </>
                  )}
                </button>
              );
            })}
          </Popover>

          {showSubmenu && submenu.kind === "setting" ? (
            <Popover
              key={submenu.setting.id}
              anchor={activeRow}
              side="right"
              gap={SUBMENU_OVERLAP}
              width={SETTING_MENU_WIDTH}
              layer={LAYER.submenu}
              role="menu"
              aria-label={settingLabel(submenu.setting)}
              onMouseEnter={() => setSubmenu(submenu)}
              data-model-picker
              className="p-1 font-sans"
            >
              {submenu.setting.options.map((option, index) => {
                const selected =
                  option.value === settingValue(submenu.setting, values);
                const highlighted = index === activeSetting;
                return (
                  <button
                    key={option.value}
                    type="button"
                    role="menuitemradio"
                    aria-checked={selected}
                    onMouseDown={(event) => event.preventDefault()}
                    onMouseEnter={() => setActiveSetting(index)}
                    onClick={() => pickSetting(submenu.setting, option.value)}
                    className={`flex h-8 w-full items-center gap-2 rounded-lg px-2 text-left text-[13px] ${
                      highlighted
                        ? "bg-content/10 text-content"
                        : "text-content hover:bg-content/5"
                    }`}
                  >
                    <span className="min-w-0 flex-1 truncate">
                      {option.label}
                    </span>
                    {selected ? (
                      <Check
                        className="size-3.5 shrink-0 text-content/50"
                        strokeWidth={2}
                      />
                    ) : null}
                  </button>
                );
              })}
            </Popover>
          ) : null}

          {showSubmenu && submenu.kind === "models" ? (
            <ModelFlyout
              anchor={activeRow}
              harnesses={pickerHarnesses}
              tab={visibleTab}
              models={visibleModels}
              currentId={current.id}
              active={activeModel}
              query={query}
              favorites={favorites}
              searchRef={search}
              onQuery={setQuery}
              onSelectTab={selectTab}
              onActive={setActiveModel}
              onPick={pickModel}
              onToggleFavorite={toggleFavorite}
            />
          ) : null}
        </>
      ) : null}
    </>
  );
}

function ModelFlyout({
  anchor,
  harnesses,
  tab,
  models,
  currentId,
  active,
  query,
  favorites,
  searchRef,
  onQuery,
  onSelectTab,
  onActive,
  onPick,
  onToggleFavorite,
}: {
  anchor: HTMLButtonElement;
  harnesses: HarnessId[];
  tab: ModelPickerTab;
  models: AgentModel[];
  currentId: string;
  active: number;
  query: string;
  favorites: string[];
  searchRef: React.RefObject<HTMLInputElement | null>;
  onQuery: (query: string) => void;
  onSelectTab: (tab: ModelPickerTab) => void;
  onActive: (index: number) => void;
  onPick: (model: AgentModel) => void;
  onToggleFavorite: (id: string) => void;
}) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const activeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const onSearchKey = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      event.stopPropagation();
      onActive(Math.min(models.length - 1, active + 1));
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      event.stopPropagation();
      onActive(Math.max(0, active - 1));
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      const item = models[active];
      if (item) onPick(item);
      return;
    }
    if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      event.stopPropagation();
    }
  };

  return (
    <Popover
      anchor={anchor}
      side="right"
      gap={SUBMENU_OVERLAP}
      width={MODEL_MENU_WIDTH}
      minHeight={MODEL_MENU_FRAME_HEIGHT}
      maxHeight={MODEL_MENU_FRAME_HEIGHT}
      layer={LAYER.submenu}
      role="dialog"
      aria-label="Models"
      data-model-picker
      style={{
        height: MODEL_MENU_HEIGHT,
        minHeight: MODEL_MENU_HEIGHT,
        maxHeight: MODEL_MENU_HEIGHT,
      }}
      className="flex min-h-0 overflow-hidden font-sans"
    >
      <nav
        role="tablist"
        aria-label="Providers"
        aria-orientation="vertical"
        className="flex w-11 shrink-0 flex-col items-center gap-1 border-r border-content/10 p-1.5"
      >
        <ProviderTabButton
          title="Favorites"
          selected={tab === "favorites"}
          onSelect={() => onSelectTab("favorites")}
        >
          <Star
            className="size-4"
            strokeWidth={1.75}
            fill={tab === "favorites" ? "currentColor" : "none"}
          />
        </ProviderTabButton>
        {harnesses.map((harness) => (
          <ProviderTabButton
            key={harness}
            title={HARNESS_TITLE[harness]}
            selected={tab === harness}
            onSelect={() => onSelectTab(harness)}
          >
            <HarnessIcon harness={harness} className="size-4" />
          </ProviderTabButton>
        ))}
      </nav>

      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <label className="flex shrink-0 items-center gap-2 border-b border-content/10 px-3 py-2.5 text-content/50">
          <Search className="size-3.5 shrink-0" strokeWidth={1.75} />
          <input
            ref={searchRef}
            type="text"
            value={query}
            placeholder="Search models"
            aria-label="Search models"
            className="min-w-0 flex-1 bg-transparent text-[13px] text-content outline-none placeholder:text-content/40"
            onChange={(event) => onQuery(event.target.value)}
            onKeyDown={onSearchKey}
          />
        </label>

        <div
          ref={lockOverscroll}
          role="listbox"
          aria-label="Models"
          className="min-h-0 flex-1 overflow-y-auto overscroll-none p-1"
        >
          {models.length === 0 ? (
            <div className="px-2 py-3 text-[12px] text-content/50">
              {tab === "favorites" && !query.trim()
                ? "No favorite models"
                : tab !== "favorites" && !isHarnessAvailable(tab)
                  ? harnessUnavailableHint(tab)
                  : tab === "codex" && !query.trim()
                    ? "Loading Codex models…"
                    : "No matching models"}
            </div>
          ) : (
            models.map((item, index) => {
              const selected = item.id === currentId;
              const highlighted = index === active;
              const favorited = favorites.includes(item.id);
              const disabled = !isHarnessAvailable(item.harness);
              return (
                <div
                  key={item.id}
                  className={`group flex h-8 items-center rounded-lg px-1 ${
                    disabled
                      ? "text-content/30"
                      : highlighted
                        ? "bg-content/10 text-content"
                        : "text-content hover:bg-content/5"
                  }`}
                  onMouseEnter={() => onActive(index)}
                >
                  <button
                    ref={highlighted ? activeRef : undefined}
                    type="button"
                    role="option"
                    aria-selected={selected}
                    disabled={disabled}
                    title={
                      disabled
                        ? harnessUnavailableHint(item.harness)
                        : undefined
                    }
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={() => onPick(item)}
                    className="flex min-w-0 flex-1 items-center gap-2 px-1.5 text-left text-[13px] disabled:cursor-not-allowed"
                  >
                    <span className="min-w-0 flex-1 truncate">{item.name}</span>
                  </button>
                  <button
                    type="button"
                    title={
                      favorited ? "Remove from favorites" : "Add to favorites"
                    }
                    aria-label={
                      favorited ? "Remove from favorites" : "Add to favorites"
                    }
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={(event) => {
                      event.stopPropagation();
                      onToggleFavorite(item.id);
                    }}
                    className={`grid size-6 shrink-0 place-items-center rounded-md transition-opacity ${
                      favorited
                        ? "text-content/60"
                        : "text-content/35 opacity-0 group-hover:opacity-100 focus:opacity-100"
                    }`}
                  >
                    <Star
                      className="size-3.5"
                      strokeWidth={1.75}
                      fill={favorited ? "currentColor" : "none"}
                    />
                  </button>
                  {selected ? (
                    <span
                      aria-hidden="true"
                      className="grid size-6 shrink-0 place-items-center"
                    >
                      <Check
                        className="size-3.5 text-content/55"
                        strokeWidth={2}
                      />
                    </span>
                  ) : null}
                </div>
              );
            })
          )}
        </div>
      </div>
    </Popover>
  );
}

function ProviderTabButton({
  title,
  selected,
  onSelect,
  children,
}: {
  title: string;
  selected: boolean;
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
      onMouseDown={(event) => event.preventDefault()}
      onMouseEnter={selected ? undefined : onSelect}
      onClick={onSelect}
      className={`grid size-8 shrink-0 place-items-center rounded-md ${
        selected
          ? "bg-content/12 text-content"
          : "text-content/45 hover:bg-content/8 hover:text-content"
      }`}
    >
      <span className="shrink-0">{children}</span>
    </button>
  );
}

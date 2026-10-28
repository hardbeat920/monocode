import "./QuickModelSelector.css";
import { QuickPermissions } from "./QuickPermissions";
import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { emit } from "@tauri-apps/api/event";
import { Check, RotateCcw, Search, Star, Zap } from "../../../shared/ui/icons";
import {
  findModel,
  getModelSnapshot,
  getPickerVisibilitySnapshot,
  loadFavoriteModels,
  modelEffortSetting,
  modelsFor,
  preferredModelSettings,
  saveFavoriteModels,
  showProviderInModelPicker,
  subscribeModels,
  subscribePickerVisibility,
  type AgentModel,
  type ModelPickerTab,
} from "../../sessions/model/models";
import {
  HARNESSES,
  HARNESS_TITLE,
  type HarnessId,
  type RuntimeMode,
} from "../../sessions/model/session";
import { HarnessIcon } from "../../sessions/ui/HarnessIcon";
import {
  filterQuickModels,
  QUICK_COMPOSER_CATALOG_REQUEST_EVENT,
} from "../model/quickComposer";

type Props = {
  model: AgentModel;
  values: Record<string, string>;
  availableHarnesses: HarnessId[] | null;
  onChange: (model: AgentModel) => void;
  onSettingsChange: (values: Record<string, string>) => void;
  onClose: () => void;
  runtimeMode: RuntimeMode;
  onRuntimeModeChange: (mode: RuntimeMode) => void;
};

/** Inline model browser: providers share the full width above search/results. */
export function QuickModelSelector({
  model,
  values,
  availableHarnesses,
  onChange,
  onSettingsChange,
  onClose,
  runtimeMode,
  onRuntimeModeChange,
}: Props) {
  const catalogVersion = useSyncExternalStore(
    subscribeModels,
    getModelSnapshot,
  );
  useSyncExternalStore(subscribePickerVisibility, getPickerVisibilitySnapshot);
  const [tab, setTab] = useState<ModelPickerTab>(model.harness);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [favorites, setFavorites] = useState(loadFavoriteModels);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const listId = useId();
  const providers = HARNESSES.filter((id) =>
    showProviderInModelPicker(
      id,
      availableHarnesses?.includes(id) ?? false,
      availableHarnesses != null,
    ),
  );
  const tabs: ModelPickerTab[] = ["favorites", ...providers];
  const visibleTab = tabs.includes(tab) ? tab : "favorites";
  const pool =
    visibleTab === "favorites"
      ? favorites
          .map(findModel)
          .filter(
            (item): item is AgentModel =>
              !!item && providers.includes(item.harness),
          )
      : modelsFor(visibleTab);
  const models = filterQuickModels(pool, query);
  const effort =
    modelEffortSetting(model) ??
    model.settings?.find(
      (setting) =>
        setting.kind === "select" &&
        /^(effort|reasoning effort|reasoning)$/i.test(setting.label),
    );
  const valueFor = (id: string, fallback: string) => values[id] ?? fallback;
  const changeSetting = (id: string, value: string) =>
    onSettingsChange({ ...values, [id]: value });
  const effortIndex = Math.max(
    0,
    effort?.options.findIndex(
      (option) => option.value === valueFor(effort.id, effort.value),
    ) ?? 0,
  );
  const fast = model.settings?.find(
    (setting) =>
      setting.id === "fast" ||
      setting.id === "serviceTier" ||
      /^fast( mode)?$/i.test(setting.label),
  );
  const fastOn = fast?.options.find(
    (option) =>
      /^(true|fast|priority)$/i.test(option.value) ||
      /^(on|fast)$/i.test(option.label),
  )?.value;
  const fastOff = fast?.options.find(
    (option) =>
      /^(false|default|standard|normal)$/i.test(option.value) ||
      /^(off|standard|normal)$/i.test(option.label),
  )?.value;
  const canToggleFast = fast != null && fastOn != null && fastOff != null;
  const fastEnabled = canToggleFast && valueFor(fast.id, fast.value) === fastOn;
  const resetSettings = () => {
    const defaults = preferredModelSettings(model);
    onSettingsChange({
      ...values,
      ...(effort ? { [effort.id]: defaults[effort.id] } : {}),
      ...(canToggleFast ? { [fast.id]: defaults[fast.id] } : {}),
    });
  };
  const enabled = (item: AgentModel) =>
    availableHarnesses?.includes(item.harness) ?? false;

  useEffect(() => {
    searchRef.current?.focus({ preventScroll: true });
  }, []);
  useEffect(() => {
    if (visibleTab !== "favorites")
      void emit(QUICK_COMPOSER_CATALOG_REQUEST_EVENT, visibleTab);
  }, [visibleTab]);

  useLayoutEffect(() => {
    const selected = models.findIndex((item) => item.id === model.id);
    setActive(selected >= 0 ? selected : 0);
  }, [visibleTab, query, catalogVersion, model.id, favorites]);

  useLayoutEffect(() => {
    const list = listRef.current;
    const row = list?.querySelector<HTMLElement>(`[data-index="${active}"]`);
    if (!list || !row) return;
    const bounds = list.getBoundingClientRect();
    const rect = row.getBoundingClientRect();
    if (rect.top < bounds.top) list.scrollTop += rect.top - bounds.top;
    else if (rect.bottom > bounds.bottom)
      list.scrollTop += rect.bottom - bounds.bottom;
  }, [active, visibleTab, query, catalogVersion, favorites]);

  const selectTab = (next: ModelPickerTab) => {
    setTab(next);
    setQuery("");
    setActive(0);
  };
  const pick = (item: AgentModel) => {
    if (enabled(item)) onChange(item);
  };

  return (
    <section
      aria-label="Model selector"
      className="flex min-h-0 flex-col border-t border-stroke"
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          onClose();
        }
      }}
    >
      <nav
        role="tablist"
        aria-label="Providers"
        className="grid h-11 shrink-0 grid-flow-col auto-cols-fr items-center gap-1 border-b border-stroke px-2"
      >
        {tabs.map((id, index) => {
          const title = id === "favorites" ? "Favorites" : HARNESS_TITLE[id];
          return (
            <button
              key={id}
              type="button"
              role="tab"
              title={title}
              aria-label={title}
              aria-selected={visibleTab === id}
              tabIndex={visibleTab === id ? 0 : -1}
              onClick={() => selectTab(id)}
              onKeyDown={(event) => {
                let next: number;
                if (event.key === "ArrowRight")
                  next = (index + 1) % tabs.length;
                else if (event.key === "ArrowLeft")
                  next = (index - 1 + tabs.length) % tabs.length;
                else if (event.key === "Home") next = 0;
                else if (event.key === "End") next = tabs.length - 1;
                else return;
                event.preventDefault();
                selectTab(tabs[next]);
                event.currentTarget.parentElement
                  ?.querySelectorAll<HTMLButtonElement>("button")
                  [next]?.focus({ preventScroll: true });
              }}
              className={`flex h-8 w-full min-w-0 items-center justify-center rounded-lg transition-colors ${visibleTab === id ? "bg-selection-emphasis text-content" : "text-content/40 hover:text-content hover:bg-selection-hover"}`}
            >
              {id === "favorites" ? (
                <Star
                  className="size-4"
                  fill={visibleTab === id ? "currentColor" : "none"}
                />
              ) : (
                <HarnessIcon harness={id} className="size-4" />
              )}
            </button>
          );
        })}
      </nav>
      <div className="flex min-h-0">
        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          <label className="flex h-10 shrink-0 items-center gap-2 border-b border-stroke px-4 text-content/40">
            <Search className="size-3.5 shrink-0" />
            <input
              ref={searchRef}
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                setActive(0);
              }}
              role="combobox"
              aria-label="Search models"
              aria-controls={listId}
              aria-expanded="true"
              aria-autocomplete="list"
              aria-activedescendant={
                models[active] ? `${listId}-${active}` : undefined
              }
              placeholder="Search models…"
              autoComplete="off"
              spellCheck={false}
              className="min-w-0 flex-1 bg-transparent text-[13px] text-content outline-none placeholder:text-content/35"
              onKeyDown={(event) => {
                if (event.nativeEvent.isComposing) return;
                if (event.key === "ArrowDown" || event.key === "ArrowUp") {
                  event.preventDefault();
                  if (models.length)
                    setActive(
                      (index) =>
                        (index +
                          (event.key === "ArrowDown" ? 1 : -1) +
                          models.length) %
                        models.length,
                    );
                } else if (event.key === "Enter") {
                  event.preventDefault();
                  const selected = models[active];
                  if (selected) pick(selected);
                }
              }}
            />
          </label>
          <div
            ref={listRef}
            id={listId}
            role="listbox"
            aria-label="Models"
            className="h-60 min-h-0 overflow-y-auto overscroll-none p-2"
          >
            {models.length ? (
              models.map((item, index) => (
                <div
                  key={`${item.harness}:${item.id}`}
                  data-index={index}
                  className={`flex h-8 items-center rounded-lg ${index === active ? "bg-selection-emphasis" : "hover:bg-selection-hover"}`}
                  onMouseEnter={() => setActive(index)}
                >
                  <button
                    id={`${listId}-${index}`}
                    type="button"
                    role="option"
                    aria-selected={item.id === model.id}
                    disabled={!enabled(item)}
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={() => pick(item)}
                    className="flex min-w-0 flex-1 items-center gap-2 px-2 text-left text-[13px] text-content disabled:opacity-35"
                  >
                    {visibleTab === "favorites" ? (
                      <HarnessIcon
                        harness={item.harness}
                        className="size-3.5 shrink-0"
                      />
                    ) : null}
                    <span className="truncate">{item.name}</span>
                    {item.provider ? (
                      <span className="ml-auto truncate text-[11px] text-content/40">
                        {item.provider.name}
                      </span>
                    ) : null}
                    {item.id === model.id ? (
                      <Check className="ml-auto size-3.5 shrink-0 text-accent" />
                    ) : null}
                  </button>
                  <button
                    type="button"
                    title={
                      favorites.includes(item.id)
                        ? "Remove from favorites"
                        : "Add to favorites"
                    }
                    aria-label={`${favorites.includes(item.id) ? "Remove" : "Add"} ${item.name} ${favorites.includes(item.id) ? "from" : "to"} favorites`}
                    onClick={() => {
                      const next = favorites.includes(item.id)
                        ? favorites.filter((id) => id !== item.id)
                        : [...favorites, item.id];
                      setFavorites(next);
                      saveFavoriteModels(next);
                    }}
                    className="grid size-8 shrink-0 place-items-center text-content/35 hover:text-content"
                  >
                    <Star
                      className="size-3.5"
                      fill={
                        favorites.includes(item.id) ? "currentColor" : "none"
                      }
                    />
                  </button>
                </div>
              ))
            ) : (
              <p className="px-2 py-6 text-center text-[12px] text-content/45">
                {query
                  ? "No matching models"
                  : visibleTab === "favorites"
                    ? "No favorite models"
                    : "Loading models…"}
              </p>
            )}
          </div>
          {effort && effort.options.length > 0 ? (
            <div className="shrink-0 border-t border-stroke px-4 pb-3 pt-2">
              <div className="mb-1 grid grid-cols-[28px_1fr_28px] items-center">
                {canToggleFast ? (
                  <button
                    type="button"
                    aria-label="Fast mode"
                    aria-pressed={fastEnabled}
                    title={
                      fastEnabled ? "Turn off fast mode" : "Turn on fast mode"
                    }
                    onClick={() =>
                      changeSetting(fast.id, fastEnabled ? fastOff : fastOn)
                    }
                    className={`grid size-7 place-items-center rounded-md transition-colors ${fastEnabled ? "bg-amber-400/20 text-amber-400 hover:bg-amber-400/30" : "text-content/40 hover:bg-selection-hover hover:text-content"}`}
                  >
                    <Zap
                      className="size-4"
                      fill={fastEnabled ? "currentColor" : "none"}
                    />
                  </button>
                ) : (
                  <span />
                )}
                <span className="text-center text-[13px] font-medium text-accent">
                  {effort.options[effortIndex]?.label}
                </span>
                <button
                  type="button"
                  aria-label="Reset to saved defaults"
                  title="Reset to saved defaults"
                  onClick={resetSettings}
                  className="grid size-7 place-items-center rounded-md text-content/40 hover:bg-selection-hover hover:text-content"
                >
                  <RotateCcw className="size-4" />
                </button>
              </div>
              <div className="relative h-6">
                <div
                  aria-hidden="true"
                  className="pointer-events-none absolute inset-x-0 top-1/2 h-3 -translate-y-1/2 rounded-full"
                  style={{
                    background: `linear-gradient(to right, var(--color-accent) ${(effortIndex / Math.max(1, effort.options.length - 1)) * 100}%, color-mix(in srgb, var(--color-content) 15%, transparent) 0%)`,
                  }}
                />
                <div
                  aria-hidden="true"
                  className="pointer-events-none absolute inset-x-2.5 top-1/2"
                >
                  {effort.options.map((option, index) => (
                    <span
                      key={option.value}
                      className={`absolute size-1 -translate-x-1/2 -translate-y-1/2 rounded-full ${index <= effortIndex ? "bg-white/45" : "bg-content/25"}`}
                      style={{
                        left: `${(index / Math.max(1, effort.options.length - 1)) * 100}%`,
                      }}
                    />
                  ))}
                </div>
                <input
                  type="range"
                  aria-label={effort.label}
                  min={0}
                  max={effort.options.length - 1}
                  step={1}
                  value={effortIndex}
                  aria-valuetext={effort.options[effortIndex]?.label}
                  onChange={(event) =>
                    changeSetting(
                      effort.id,
                      effort.options[Number(event.target.value)].value,
                    )
                  }
                  className="quick-reasoning-slider relative z-10 block h-6 w-full"
                />
              </div>
            </div>
          ) : null}
        </div>
        {model.harness !== "fx" ? (
          <aside className="flex min-h-0 w-1/2 shrink-0 flex-col border-l border-stroke">
            <h3 className="flex h-10 shrink-0 items-center border-b border-stroke px-4 text-[12px] font-medium text-content/55">
              Permissions
            </h3>
            <QuickPermissions
              embedded
              value={runtimeMode}
              onChange={onRuntimeModeChange}
              onClose={onClose}
            />
          </aside>
        ) : null}
      </div>
    </section>
  );
}

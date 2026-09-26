import {
  Check,
  ChevronDown,
  ChevronRight,
  MessageMultiple,
  Replace,
  type IconComponent,
} from "../../../shared/ui/icons";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import {
  getHarnessAvailabilitySnapshot,
  hasProbedHarnessAvailability,
  isHarnessAvailable,
  probeHarnessAvailability,
  subscribeHarnessAvailability,
} from "../../../integrations/harness/core/availability";
import { refreshHarnessCatalogs } from "../../../integrations/harness/core/registry";
import { useLockOverscroll } from "../../../shared/hooks/useLockOverscroll";
import {
  getModelSnapshot,
  getPickerVisibilitySnapshot,
  isPickerProviderVisible,
  mergeModelSettings,
  modelEffortSetting,
  modelsFor,
  preferredModelId,
  subscribeModels,
  subscribePickerVisibility,
} from "../model/models";
import { LAYER } from "../../../shared/lib/layers";
import { secondOpinionTargets } from "../model/secondOpinion";
import {
  HARNESS_TITLE,
  type HarnessId,
  type ModelTarget,
} from "../model/session";
import { HarnessIcon } from "./HarnessIcon";
import { Popover } from "../../../shared/ui/Popover";

type Props = {
  from: HarnessId;
  fromModel?: string;
  fromSettings?: Record<string, string>;
  onPick: (target: ModelTarget) => void;
  icon?: IconComponent;
  title?: string;
  disabledTitle?: string;
  description?: string;
  menuLabel?: string;
  includeCurrent?: boolean;
  /**
   * Hide `fromModel` from `from`'s own model list instead of offering it
   * back as a target. Only the plain "Second opinion" button sets this —
   * `BuildTargetButton` still needs to reselect the same model to change
   * its effort, so it leaves this off.
   */
  excludeFromModel?: boolean;
  disabled?: boolean;
  triggerClassName?: string;
};

const MENU_WIDTH = 240;
const SUBMENU_WIDTH = 240;
const SUBMENU_MAX_HEIGHT = 288;
const EFFORT_MENU_WIDTH = 200;
/** The flyout tucks under the parent menu's edge rather than floating free. */
const SUBMENU_OVERLAP = -4;
/** Neither menu is inside the other, so a click in one is not a click away. */
const SELF = "[data-provider-target]";

export function HandoffButton({
  from,
  onPick,
}: Pick<Props, "from" | "onPick">) {
  return (
    <SecondOpinionButton
      from={from}
      onPick={onPick}
      icon={Replace}
      title="Handoff"
      disabledTitle="Install another provider to hand off"
      description="Hand this session to another agent to continue the work."
      menuLabel="Hand this session to another agent"
    />
  );
}

export function BuildTargetButton({
  from,
  model,
  settings,
  disabled,
  onPick,
}: {
  from: HarnessId;
  model?: string;
  settings?: Record<string, string>;
  disabled?: boolean;
  onPick: (target: ModelTarget) => void;
}) {
  return (
    <SecondOpinionButton
      from={from}
      fromModel={model}
      fromSettings={settings}
      onPick={onPick}
      icon={ChevronDown}
      title="Build with another model"
      disabledTitle="No build providers are available"
      description="Choose the model and provider that should build this plan."
      menuLabel="Build this plan with another model or provider"
      includeCurrent
      disabled={disabled}
      triggerClassName="flex h-6 w-6 shrink-0 items-center justify-center rounded-r-md border-l border-background-base/20 bg-content text-background-base hover:bg-content/90 disabled:pointer-events-none disabled:opacity-40"
    />
  );
}

export function SecondOpinionButton({
  from,
  fromModel,
  fromSettings,
  onPick,
  icon: Icon = MessageMultiple,
  title = "Second opinion",
  disabledTitle = "Install another provider for a second opinion",
  description = "Send this turn to another agent to review the work.",
  menuLabel = "Send this turn to another agent",
  includeCurrent = false,
  excludeFromModel = false,
  disabled: disabledByCaller = false,
  triggerClassName,
}: Props) {
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
  const catalogVersion = useSyncExternalStore(
    subscribeModels,
    getModelSnapshot,
    getModelSnapshot,
  );
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [modelActive, setModelActive] = useState(0);
  const [effortActive, setEffortActive] = useState(0);
  const [menuLevel, setMenuLevel] = useState<"providers" | "models" | "effort">(
    "providers",
  );
  const button = useRef<HTMLButtonElement>(null);
  const [activeRow, setActiveRow] = useState<HTMLButtonElement | null>(null);
  const [activeModelRow, setActiveModelRow] =
    useState<HTMLButtonElement | null>(null);
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();

  const probed = hasProbedHarnessAvailability();
  const targets = useMemo(() => {
    void availabilityVersion;
    void visibilityVersion;
    return secondOpinionTargets(from, {
      installed: isHarnessAvailable,
      visible: isPickerProviderVisible,
      probed,
      includeCurrent,
    });
  }, [from, includeCurrent, probed, availabilityVersion, visibilityVersion]);

  const activeHarness = targets[active];
  const models = useMemo(() => {
    void catalogVersion;
    if (!activeHarness) return [];
    const list = modelsFor(activeHarness);
    // The current model is not a second opinion on itself, so it is not an
    // option once the picker lets the turn's own harness back in.
    return excludeFromModel && activeHarness === from && fromModel
      ? list.filter((model) => model.id !== fromModel)
      : list;
  }, [activeHarness, catalogVersion, from, fromModel, excludeFromModel]);
  const preferred =
    activeHarness != null
      ? activeHarness === from && fromModel && !excludeFromModel
        ? fromModel
        : preferredModelId(activeHarness)
      : undefined;
  const activeModel = models[modelActive];
  const activeEffort = activeModel
    ? modelEffortSetting(activeModel)
    : undefined;
  const selectedEffortValue = activeEffort
    ? activeModel.id === fromModel
      ? (fromSettings?.[activeEffort.id] ?? activeEffort.value)
      : activeEffort.value
    : undefined;

  useEffect(() => {
    if (!open) return;
    void probeHarnessAvailability();
  }, [open]);

  useEffect(() => {
    if (!open || !activeHarness) return;
    void refreshHarnessCatalogs([activeHarness]);
  }, [open, activeHarness]);

  useEffect(() => {
    setActive(0);
    setMenuLevel("providers");
  }, [open, targets.join(",")]);

  useEffect(() => {
    if (!activeHarness) {
      setModelActive(0);
      return;
    }
    const index = models.findIndex((model) => model.id === preferred);
    setModelActive(index >= 0 ? index : 0);
  }, [activeHarness, preferred, models]);

  useEffect(() => {
    if (!activeEffort) {
      setEffortActive(0);
      return;
    }
    const index = activeEffort.options.findIndex(
      (option) => option.value === selectedEffortValue,
    );
    setEffortActive(index >= 0 ? index : 0);
  }, [activeEffort, selectedEffortValue]);

  useEffect(() => {
    activeModelRow?.scrollIntoView({ block: "nearest" });
  }, [activeModelRow]);

  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    window.addEventListener("resize", close);
    return () => window.removeEventListener("resize", close);
  }, [open]);

  const dismiss = (restoreFocus: boolean) => {
    setOpen(false);
    if (restoreFocus) button.current?.focus();
  };

  // With the current model hidden, a target harness only counts as usable
  // if it still has a model left to offer once that exclusion is applied.
  const hasSelectableModel =
    !excludeFromModel || !fromModel
      ? true
      : targets.some((harness) => {
          const list = modelsFor(harness);
          return harness === from
            ? list.some((model) => model.id !== fromModel)
            : list.length > 0;
        });
  const noTargets = targets.length === 0 || !hasSelectableModel;
  const disabled = disabledByCaller || noTargets;
  const label = noTargets ? disabledTitle : title;

  const pick = (model: (typeof models)[number], effortValue?: string) => {
    const effort = modelEffortSetting(model);
    const modelSettings = mergeModelSettings(model, {
      ...(model.id === fromModel ? fromSettings : undefined),
      ...(effort && effortValue ? { [effort.id]: effortValue } : {}),
    });
    setOpen(false);
    onPick({ harness: model.harness, model: model.id, modelSettings });
  };

  const openModels = () => {
    if (models.length > 0) setMenuLevel("models");
  };

  const openEffortOrPick = (model: (typeof models)[number]) => {
    if (modelEffortSetting(model)?.options.length) {
      setMenuLevel("effort");
    } else {
      pick(model);
    }
  };

  const moveHarness = (dir: 1 | -1) => {
    if (targets.length === 0) return;
    setMenuLevel("providers");
    setActive((index) => (index + dir + targets.length) % targets.length);
  };

  const moveModel = (dir: 1 | -1) => {
    if (models.length === 0) return;
    setModelActive((index) => (index + dir + models.length) % models.length);
  };

  const moveEffort = (dir: 1 | -1) => {
    if (!activeEffort?.options.length) return;
    setEffortActive(
      (index) =>
        (index + dir + activeEffort.options.length) %
        activeEffort.options.length,
    );
  };

  const onMenuKey = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      if (menuLevel === "effort") moveEffort(1);
      else if (menuLevel === "models") moveModel(1);
      else moveHarness(1);
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      if (menuLevel === "effort") moveEffort(-1);
      else if (menuLevel === "models") moveModel(-1);
      else moveHarness(-1);
      return;
    }
    if (event.key === "ArrowRight") {
      event.preventDefault();
      if (menuLevel === "providers") openModels();
      else if (menuLevel === "models" && activeEffort?.options.length)
        setMenuLevel("effort");
      return;
    }
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      setMenuLevel(menuLevel === "effort" ? "models" : "providers");
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      if (!activeHarness) return;
      if (menuLevel === "effort") {
        const option = activeEffort?.options[effortActive];
        if (activeModel && option) pick(activeModel, option.value);
        return;
      }
      if (menuLevel === "models") {
        if (activeModel) openEffortOrPick(activeModel);
        return;
      }
      openModels();
    }
  };

  const showSubmenu =
    open &&
    menuLevel !== "providers" &&
    activeRow != null &&
    activeRow.dataset.providerIndex === String(active) &&
    activeHarness != null &&
    models.length > 0;
  const showEffort =
    showSubmenu &&
    menuLevel === "effort" &&
    activeModelRow != null &&
    activeModelRow.dataset.modelIndex === String(modelActive) &&
    activeModel != null &&
    activeEffort != null &&
    activeEffort.options.length > 0;

  return (
    <>
      <button
        ref={button}
        type="button"
        title={label}
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={disabled}
        className={
          triggerClassName ??
          `rounded-md p-1 disabled:pointer-events-none disabled:opacity-40 ${
            open
              ? "bg-content/8 text-content/70"
              : "text-content/40 hover:bg-content/8 hover:text-content/70"
          }`
        }
        onClick={() => {
          if (disabled) return;
          setOpen((value) => !value);
        }}
      >
        <Icon className="size-3.5" strokeWidth={1.75} />
      </button>
      {open ? (
        <>
          <Popover
            anchor={button}
            side="top"
            align="center"
            width={MENU_WIDTH}
            autoFocus
            ignore={SELF}
            onDismiss={(reason) => dismiss(reason === "escape")}
            role="menu"
            tabIndex={-1}
            aria-label={menuLabel}
            onKeyDown={onMenuKey}
            data-provider-target
            className="p-1 font-sans"
          >
            <div className="px-1.5 pb-2 pt-1.5">
              <p className="text-[11px] leading-3 text-content/50 text-balance">
                {description}
              </p>
            </div>
            <div className="mx-1 mb-1 h-px bg-content/10" />
            {targets.length === 0 ? (
              <div className="px-2.5 py-2 text-[12px] leading-4 text-content/50">
                {disabledTitle}
              </div>
            ) : (
              targets.map((harness, index) => {
                const highlighted = index === active;
                const available = isHarnessAvailable(harness);
                return (
                  <button
                    key={harness}
                    ref={highlighted ? setActiveRow : undefined}
                    data-provider-index={index}
                    type="button"
                    role="menuitem"
                    aria-haspopup={
                      modelsFor(harness).length > 0 ? "menu" : undefined
                    }
                    aria-expanded={highlighted && showSubmenu}
                    disabled={!available && probed}
                    onMouseDown={(event) => event.preventDefault()}
                    onMouseEnter={() => {
                      setActive(index);
                      setMenuLevel("models");
                    }}
                    onClick={() => {
                      if (!available && probed) return;
                      setActive(index);
                      if (modelsFor(harness).length > 0) setMenuLevel("models");
                    }}
                    className={`flex h-8 w-full items-center gap-2 rounded-lg px-2 text-left text-[13px] leading-none ${
                      !available && probed
                        ? "text-content/30"
                        : highlighted
                          ? "bg-selection text-content"
                          : "text-content hover:bg-content/5"
                    }`}
                  >
                    <HarnessIcon harness={harness} className="size-3.5" />
                    <span className="min-w-0 flex-1 truncate">
                      {HARNESS_TITLE[harness]}
                    </span>
                    {modelsFor(harness).length > 0 ? (
                      <ChevronRight
                        className="size-3.5 shrink-0 text-content/40"
                        strokeWidth={1.75}
                      />
                    ) : null}
                  </button>
                );
              })
            )}
          </Popover>
          {showSubmenu ? (
            <Popover
              // Remounting per row re-measures the flyout against that row.
              key={active}
              ref={lockOverscroll}
              anchor={activeRow}
              side="right"
              gap={SUBMENU_OVERLAP}
              width={SUBMENU_WIDTH}
              maxHeight={SUBMENU_MAX_HEIGHT}
              layer={LAYER.submenu}
              role="menu"
              aria-label={`${HARNESS_TITLE[activeHarness]} models`}
              ignore={SELF}
              onMouseEnter={() =>
                setMenuLevel((level) =>
                  level === "providers" ? "models" : level,
                )
              }
              data-provider-target
              className="overflow-y-auto overscroll-none p-1"
            >
              {models.map((model, index) => {
                const highlighted = index === modelActive;
                return (
                  <button
                    key={model.id}
                    ref={highlighted ? setActiveModelRow : undefined}
                    data-model-index={index}
                    type="button"
                    role="menuitem"
                    aria-haspopup={
                      modelEffortSetting(model)?.options.length
                        ? "menu"
                        : undefined
                    }
                    aria-expanded={highlighted && showEffort}
                    onMouseDown={(event) => event.preventDefault()}
                    onMouseEnter={() => {
                      setModelActive(index);
                      setMenuLevel(
                        modelEffortSetting(model)?.options.length
                          ? "effort"
                          : "models",
                      );
                    }}
                    onClick={() => {
                      setModelActive(index);
                      openEffortOrPick(model);
                    }}
                    className={`flex h-8 w-full items-center gap-2 rounded-lg px-2 text-left text-[13px] leading-none ${
                      highlighted
                        ? "bg-selection text-content"
                        : "text-content hover:bg-content/5"
                    }`}
                  >
                    <span className="min-w-0 flex-1 truncate">
                      {model.name}
                    </span>
                    {model.id === preferred ? (
                      <Check
                        className="size-3 shrink-0 text-content/45"
                        strokeWidth={2}
                      />
                    ) : null}
                    {modelEffortSetting(model)?.options.length ? (
                      <ChevronRight
                        className="size-3.5 shrink-0 text-content/40"
                        strokeWidth={1.75}
                      />
                    ) : null}
                  </button>
                );
              })}
            </Popover>
          ) : null}
          {showEffort ? (
            <Popover
              key={`${activeModel.id}:effort`}
              anchor={activeModelRow}
              side="right"
              gap={SUBMENU_OVERLAP}
              width={EFFORT_MENU_WIDTH}
              layer={LAYER.submenu + 1}
              role="menu"
              aria-label={`${activeModel.name} effort`}
              ignore={SELF}
              onMouseEnter={() => setMenuLevel("effort")}
              data-provider-target
              className="p-1 font-sans"
            >
              {activeEffort.options.map((option, index) => {
                const highlighted = index === effortActive;
                const selected = option.value === selectedEffortValue;
                return (
                  <button
                    key={option.value}
                    type="button"
                    role="menuitemradio"
                    aria-checked={selected}
                    onMouseDown={(event) => event.preventDefault()}
                    onMouseEnter={() => setEffortActive(index)}
                    onClick={() => pick(activeModel, option.value)}
                    className={`flex h-8 w-full items-center gap-2 rounded-lg px-2 text-left text-[13px] text-content ${
                      highlighted ? "bg-selection" : "hover:bg-content/5"
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
        </>
      ) : null}
    </>
  );
}

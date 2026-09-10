import {
  Check,
  ChevronDown,
  ChevronRight,
  MessageMultiple,
  Replace,
  type IconComponent,
} from "./icons";
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
} from "../lib/harness/availability";
import { refreshHarnessCatalogs } from "../lib/harness/registry";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import {
  getModelSnapshot,
  getPickerVisibilitySnapshot,
  isPickerProviderVisible,
  modelsFor,
  preferredModelId,
  subscribeModels,
  subscribePickerVisibility,
} from "../lib/models";
import { LAYER } from "../lib/layers";
import { secondOpinionTargets } from "../lib/secondOpinion";
import { HARNESS_TITLE, type HarnessId } from "../lib/session";
import { HarnessIcon } from "./HarnessIcon";
import { Popover } from "./Popover";

type Props = {
  from: HarnessId;
  fromModel?: string;
  onPick: (harness: HarnessId, model: string) => void;
  icon?: IconComponent;
  title?: string;
  disabledTitle?: string;
  description?: string;
  menuLabel?: string;
  includeCurrent?: boolean;
  disabled?: boolean;
  triggerClassName?: string;
};

const MENU_WIDTH = 240;
const SUBMENU_WIDTH = 240;
const SUBMENU_MAX_HEIGHT = 288;
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
  disabled,
  onPick,
}: {
  from: HarnessId;
  model?: string;
  disabled?: boolean;
  onPick: (harness: HarnessId, model: string) => void;
}) {
  return (
    <SecondOpinionButton
      from={from}
      fromModel={model}
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
  onPick,
  icon: Icon = MessageMultiple,
  title = "Second opinion",
  disabledTitle = "Install another provider for a second opinion",
  description = "Send this turn to another agent to review the work.",
  menuLabel = "Send this turn to another agent",
  includeCurrent = false,
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
  const [inSubmenu, setInSubmenu] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const [activeRow, setActiveRow] = useState<HTMLButtonElement | null>(null);
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
    return activeHarness ? modelsFor(activeHarness) : [];
  }, [activeHarness, catalogVersion]);
  const preferred =
    activeHarness != null
      ? activeHarness === from && fromModel
        ? fromModel
        : preferredModelId(activeHarness)
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
    setInSubmenu(false);
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
    if (!open) return;
    const close = () => setOpen(false);
    window.addEventListener("resize", close);
    return () => window.removeEventListener("resize", close);
  }, [open]);

  const dismiss = (restoreFocus: boolean) => {
    setOpen(false);
    if (restoreFocus) button.current?.focus();
  };

  const noTargets = targets.length === 0;
  const disabled = disabledByCaller || noTargets;
  const label = noTargets ? disabledTitle : title;

  const pick = (harness: HarnessId, model: string) => {
    setOpen(false);
    onPick(harness, model);
  };

  const pickPreferred = (harness: HarnessId) => {
    pick(
      harness,
      harness === from && fromModel ? fromModel : preferredModelId(harness),
    );
  };

  const moveHarness = (dir: 1 | -1) => {
    if (targets.length === 0) return;
    setInSubmenu(false);
    setActive((index) => (index + dir + targets.length) % targets.length);
  };

  const moveModel = (dir: 1 | -1) => {
    if (models.length === 0) return;
    setModelActive((index) => (index + dir + models.length) % models.length);
  };

  const onMenuKey = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      if (inSubmenu) moveModel(1);
      else moveHarness(1);
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      if (inSubmenu) moveModel(-1);
      else moveHarness(-1);
      return;
    }
    if (event.key === "ArrowRight") {
      event.preventDefault();
      if (!inSubmenu && models.length > 0) setInSubmenu(true);
      return;
    }
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      setInSubmenu(false);
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      if (!activeHarness) return;
      if (inSubmenu) {
        const model = models[modelActive];
        if (model) pick(activeHarness, model.id);
        return;
      }
      pickPreferred(activeHarness);
    }
  };

  const showSubmenu =
    open &&
    inSubmenu &&
    activeRow != null &&
    activeRow.dataset.providerIndex === String(active) &&
    activeHarness != null &&
    models.length > 0;

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
                      setInSubmenu(true);
                    }}
                    onClick={() => {
                      if (!available && probed) return;
                      pickPreferred(harness);
                    }}
                    className={`flex h-8 w-full items-center gap-2 rounded-lg px-2 text-left text-[13px] leading-none ${
                      !available && probed
                        ? "text-content/30"
                        : highlighted
                          ? "bg-content/10 text-content"
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
              onMouseEnter={() => setInSubmenu(true)}
              data-provider-target
              className="overflow-y-auto overscroll-none p-1"
            >
              {models.map((model, index) => {
                const highlighted = index === modelActive;
                return (
                  <button
                    key={model.id}
                    type="button"
                    role="menuitem"
                    onMouseDown={(event) => event.preventDefault()}
                    onMouseEnter={() => {
                      setInSubmenu(true);
                      setModelActive(index);
                    }}
                    onClick={() => pick(activeHarness, model.id)}
                    className={`flex h-8 w-full items-center gap-2 rounded-lg px-2 text-left text-[13px] leading-none ${
                      highlighted
                        ? "bg-content/10 text-content"
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

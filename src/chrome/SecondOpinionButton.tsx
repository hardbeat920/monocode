import { Check, ChevronRight, Scale } from "lucide-react";
import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { createPortal } from "react-dom";
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
import { secondOpinionTargets } from "../lib/secondOpinion";
import { HARNESS_TITLE, type HarnessId } from "../lib/session";
import {
  placeFlyoutMenu,
  placeSelectionMenu,
} from "../lib/transcriptSelection";
import { HarnessIcon } from "./HarnessIcon";

type Props = {
  from: HarnessId;
  onPick: (harness: HarnessId, model: string) => void;
};

const MENU_WIDTH = 240;
const SUBMENU_WIDTH = 240;

export function SecondOpinionButton({ from, onPick }: Props) {
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
  const [pos, setPos] = useState<CSSProperties>({
    left: 0,
    top: 0,
    visibility: "hidden",
  });
  const [subPos, setSubPos] = useState<CSSProperties>({
    left: 0,
    top: 0,
    visibility: "hidden",
  });
  const button = useRef<HTMLButtonElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const submenu = useRef<HTMLDivElement>(null);
  const rowEls = useRef(new Map<number, HTMLButtonElement>());
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();

  const probed = hasProbedHarnessAvailability();
  const targets = useMemo(() => {
    void availabilityVersion;
    void visibilityVersion;
    return secondOpinionTargets(from, {
      installed: isHarnessAvailable,
      visible: isPickerProviderVisible,
      probed,
    });
  }, [from, probed, availabilityVersion, visibilityVersion]);

  const activeHarness = targets[active];
  const models = useMemo(() => {
    void catalogVersion;
    return activeHarness ? modelsFor(activeHarness) : [];
  }, [activeHarness, catalogVersion]);
  const preferred =
    activeHarness != null ? preferredModelId(activeHarness) : undefined;

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

  useLayoutEffect(() => {
    if (!open) return;
    const anchor = button.current?.getBoundingClientRect();
    const el = panel.current;
    if (!anchor || !el) return;
    const next = placeSelectionMenu(
      anchor,
      { width: el.offsetWidth, height: el.offsetHeight },
      { width: window.innerWidth, height: window.innerHeight },
    );
    setPos({ left: next.left, top: next.top, visibility: "visible" });
  }, [open, targets.length]);

  useLayoutEffect(() => {
    if (!open || !activeHarness || models.length === 0) {
      setSubPos((current) =>
        current.visibility === "hidden"
          ? current
          : { ...current, visibility: "hidden" },
      );
      return;
    }
    const parent = panel.current?.getBoundingClientRect();
    const row = rowEls.current.get(active)?.getBoundingClientRect();
    const el = submenu.current;
    if (!parent || !row || !el) return;
    const next = placeFlyoutMenu(
      parent,
      row.top,
      { width: el.offsetWidth, height: el.offsetHeight },
      { width: window.innerWidth, height: window.innerHeight },
    );
    setSubPos({ left: next.left, top: next.top, visibility: "visible" });
  }, [open, active, activeHarness, models.length, catalogVersion, pos]);

  useEffect(() => {
    if (!open) return;
    panel.current?.focus();
    const onPointerDown = (event: PointerEvent) => {
      if (
        root.current?.contains(event.target as Node) ||
        button.current?.contains(event.target as Node)
      ) {
        return;
      }
      setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      setOpen(false);
      button.current?.focus();
    };
    const close = () => setOpen(false);
    window.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("resize", close);
    };
  }, [open]);

  const disabled = targets.length === 0;
  const title = disabled
    ? "Install another provider for a second opinion"
    : "Second opinion";

  const pick = (harness: HarnessId, model: string) => {
    setOpen(false);
    onPick(harness, model);
  };

  const pickPreferred = (harness: HarnessId) => {
    pick(harness, preferredModelId(harness));
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

  const showSubmenu = open && activeHarness != null && models.length > 0;

  return (
    <>
      <button
        ref={button}
        type="button"
        title={title}
        aria-label={title}
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={disabled}
        className={`rounded-md p-1 disabled:pointer-events-none disabled:opacity-40 ${
          open
            ? "bg-content/8 text-content/70"
            : "text-content/40 hover:bg-content/8 hover:text-content/70"
        }`}
        onClick={() => {
          if (disabled) return;
          setOpen((value) => !value);
        }}
      >
        <Scale className="size-3.5" strokeWidth={1.75} />
      </button>
      {open
        ? createPortal(
            <div ref={root}>
              <div
                ref={panel}
                role="menu"
                tabIndex={-1}
                aria-label="Send this turn to another agent"
                onKeyDown={onMenuKey}
                style={{
                  position: "fixed",
                  ...pos,
                  width: MENU_WIDTH,
                  zIndex: 80,
                }}
                className="rounded-xl border border-content/10 bg-content/10 p-1 font-sans shadow-xl backdrop-blur-xl outline-none"
              >
                <div className="px-1.5 pb-2 pt-1.5">
                  <p className="text-[11px] leading-3 text-content/50 text-balance">
                    Send this turn to another agent to review the work.
                  </p>
                </div>
                <div className="mx-1 mb-1 h-px bg-content/10" />
                {targets.length === 0 ? (
                  <div className="px-2.5 py-2 text-[12px] leading-4 text-content/50">
                    Install another provider to send this turn over.
                  </div>
                ) : (
                  targets.map((harness, index) => {
                    const highlighted = index === active;
                    const available = isHarnessAvailable(harness);
                    return (
                      <button
                        key={harness}
                        ref={(el) => {
                          if (el) rowEls.current.set(index, el);
                          else rowEls.current.delete(index);
                        }}
                        type="button"
                        role="menuitem"
                        aria-haspopup={
                          modelsFor(harness).length > 0 ? "menu" : undefined
                        }
                        aria-expanded={highlighted && models.length > 0}
                        disabled={!available && probed}
                        onMouseDown={(event) => event.preventDefault()}
                        onMouseEnter={() => {
                          setActive(index);
                          setInSubmenu(false);
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
              </div>
              {showSubmenu ? (
                <div
                  ref={(el) => {
                    submenu.current = el;
                    lockOverscroll(el);
                  }}
                  role="menu"
                  aria-label={`${HARNESS_TITLE[activeHarness]} models`}
                  onMouseEnter={() => setInSubmenu(true)}
                  style={{
                    position: "fixed",
                    ...subPos,
                    width: SUBMENU_WIDTH,
                    zIndex: 81,
                  }}
                  className="max-h-72 overflow-y-auto overscroll-none rounded-xl border border-content/10 bg-content/10 p-1 shadow-xl backdrop-blur-xl outline-none"
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
                </div>
              ) : null}
            </div>,
            document.body,
          )
        : null}
    </>
  );
}

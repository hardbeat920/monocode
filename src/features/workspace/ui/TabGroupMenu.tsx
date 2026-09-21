import {
  AppWindow,
  ChevronRight,
  ImagePlus,
  SquarePlus,
  Trash2,
  Ungroup,
  X,
  type IconComponent,
} from "../../../shared/ui/icons";
import {
  Fragment,
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { normalizeHex } from "../../../shared/lib/colorUtils";
import { projectKey } from "../../../shared/lib/paths";
import { clearProjectLogo, pickAndSetProjectLogo } from "../../projects/model/projectLogos";
import { PROJECT_MASCOTS, projectMascot } from "../../projects/model/projectMascots";
import { TAB_GROUP_COLORS } from "../model/tabGroups";
import { ColorPickerPopover, ColorSwatchRow } from "../../../shared/ui/ColorPickerPopover";
import { Popover } from "../../../shared/ui/Popover";
import { ProjectLogoIcon } from "../../projects/ui/ProjectLogoIcon";
import { ProjectMascot } from "../../projects/ui/ProjectMascot";
import { MOD } from "../../../platform/tauri/platform";
import { ExplorerMenu, type ExplorerMenuItem } from "../../files/ui/ExplorerMenu";

export type TabGroupMenuAction =
  | "new-tab"
  | "new-window"
  | "close-group"
  | "ungroup"
  | "delete-group";

export type TabGroupMenuExtraItem = {
  id: string;
  label: string;
  description?: string;
  icon: IconComponent;
  danger?: boolean;
  sepBefore?: boolean;
  disabled?: boolean;
  submenu?: ExplorerMenuItem[];
};

type Props = {
  x: number;
  y: number;
  groupId: string;
  label: string;
  colorIndex: number | null;
  customColor: string | null;
  currentColor: string;
  logoPath: string | null;
  /** Original project directory for the logo picker. */
  logoProject?: string | null;
  /** Explicit mascot pick; null means the one hashed from `mascotProject`. */
  mascotName: string | null;
  /** Key the fallback mascot is hashed from — same one the icon uses. */
  mascotProject: string;
  onRename: (groupId: string, label: string) => void;
  onColorChange: (groupId: string, colorIndex: number | null) => void;
  onCustomColorChange: (groupId: string, color: string) => void;
  onMascotChange: (groupId: string, name: string | null) => void;
  onLogoChange: () => void;
  onPick: (action: TabGroupMenuAction) => void;
  onClose: () => void;
  ariaLabel?: string;
  /** When false, only name / logo / color controls are shown. */
  showActions?: boolean;
  /** Active state action shown before the name and appearance controls. */
  leadingAction?: Pick<TabGroupMenuExtraItem, "id" | "label" | "description" | "icon">;
  extraItems?: TabGroupMenuExtraItem[];
  /** Return false to keep the menu open after validation or persistence errors. */
  onExtraPick?: (id: string) => void | boolean;
  footer?: ReactNode;
};

const MENU_WIDTH = 260;

type MenuItem = {
  id: string;
  label: string;
  shortcut?: string;
  danger?: boolean;
  icon: IconComponent;
};

const ITEMS: MenuItem[] = [
  {
    id: "new-tab",
    label: "New tab in group",
    shortcut: `${MOD}T`,
    icon: SquarePlus,
  },
  {
    id: "new-window",
    label: "Move group to new window",
    icon: AppWindow,
  },
  {
    id: "close-group",
    label: "Close group",
    shortcut: `${MOD}W`,
    icon: X,
  },
  {
    id: "ungroup",
    label: "Ungroup",
    icon: Ungroup,
  },
  {
    id: "delete-group",
    label: "Delete group",
    danger: true,
    icon: Trash2,
  },
];

export function TabGroupMenu({
  x,
  y,
  groupId,
  label,
  colorIndex,
  customColor,
  currentColor,
  logoPath,
  logoProject,
  mascotName,
  mascotProject,
  onRename,
  onColorChange,
  onCustomColorChange,
  onMascotChange,
  onLogoChange,
  onPick,
  onClose,
  ariaLabel = "Tab group actions",
  showActions = true,
  leadingAction,
  extraItems,
  onExtraPick,
  footer,
}: Props) {
  const menuId = useId();
  const [submenu, setSubmenu] = useState<{
    item: TabGroupMenuExtraItem;
    anchor: HTMLButtonElement;
  } | null>(null);
  const submenuCloseTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const cancelSubmenuClose = () => {
    if (submenuCloseTimer.current != null) clearTimeout(submenuCloseTimer.current);
    submenuCloseTimer.current = null;
  };
  const closeSubmenu = () => {
    cancelSubmenuClose();
    submenu?.anchor.focus();
    setSubmenu(null);
  };
  const scheduleSubmenuClose = () => {
    cancelSubmenuClose();
    if (submenu) submenuCloseTimer.current = setTimeout(closeSubmenu, 180);
  };
  useEffect(() => cancelSubmenuClose, [submenu]);
  const pickExtra = (id: string) => {
    if (onExtraPick?.(id) !== false) onClose();
  };
  const input = useRef<HTMLInputElement>(null);
  const [name, setName] = useState(label);
  const [customPickerOpen, setCustomPickerOpen] = useState(false);

  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);

  const shownMascot = projectMascot(mascotProject, mascotName).name;

  const commitName = () => {
    onRename(groupId, name.trim());
  };

  const onMenuKey = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Enter" && e.target === input.current) {
      e.preventDefault();
      commitName();
      onClose();
    }
  };

  return (
    <>
      <Popover
        anchor={{ x, y }}
        side="right"
        gap={0}
        width={MENU_WIDTH}
        constrainHeight={false}
        onDismiss={(reason) => {
          if (reason === "escape" && submenu) closeSubmenu();
          else onClose();
        }}
        ignore={`[data-menu-owner="${menuId}"]`}
        data-menu-owner={menuId}
        role="menu"
        tabIndex={-1}
        aria-label={ariaLabel}
        onKeyDown={onMenuKey}
        onContextMenu={(e) => e.preventDefault()}
        onMouseEnter={cancelSubmenuClose}
        onMouseLeave={scheduleSubmenuClose}
        className="p-2"
      >
        {leadingAction ? (
          <>
            <MenuRow
              item={leadingAction}
              onHover={() => setSubmenu(null)}
              onPick={() => pickExtra(leadingAction.id)}
            />
            <div role="separator" className="my-1 h-px bg-content/10" />
          </>
        ) : null}
        <input
          ref={input}
          value={name}
          onChange={(e) => setName(e.target.value)}
          onBlur={commitName}
          aria-label="Group name"
          className="mb-2 w-full rounded-lg border border-content/10 bg-content/5 px-2.5 py-1.5 text-[13px] text-content outline-none ring-accent/40 focus:ring-1"
        />

        {logoProject ? (
          <div className="mb-2 flex items-center gap-2 px-0.5">
            <button
              type="button"
              title={logoPath ? "Change project logo" : "Add project logo"}
              aria-label={logoPath ? "Change project logo" : "Add project logo"}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                void (async () => {
                  try {
                    const path = await pickAndSetProjectLogo(logoProject);
                    if (path) onLogoChange();
                  } catch (error) {
                    console.error("Failed to save project logo:", error);
                  } finally {
                    onClose();
                  }
                })();
              }}
              className="grid size-9 shrink-0 place-items-center rounded-lg border border-content/10 bg-content/5 hover:bg-content/10"
            >
              <ProjectLogoIcon
                path={logoPath}
                className="size-5"
                imageClassName="size-5"
                fallback={ImagePlus}
                fallbackStrokeWidth={1.75}
              />
            </button>
            <div className="min-w-0 flex-1">
              <p className="text-[11px] text-content/50">Project logo</p>
              <p className="truncate text-[12px] text-content/70">
                {logoPath ? "Shown in tabs and composer" : "Optional — replaces folder icon"}
              </p>
            </div>
            {logoPath ? (
              <button
                type="button"
                title="Remove project logo"
                aria-label="Remove project logo"
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => {
                  void clearProjectLogo(projectKey(logoProject)).then(onLogoChange);
                }}
                className="grid size-7 shrink-0 place-items-center rounded-md text-content/50 hover:bg-content/10 hover:text-content"
              >
                <Trash2 className="size-3.5" strokeWidth={1.75} />
              </button>
            ) : null}
          </div>
        ) : null}

        <div className="mb-2">
          <ColorSwatchRow
            colors={TAB_GROUP_COLORS}
            colorIndex={colorIndex}
            customColor={customColor}
            customPickerOpen={customPickerOpen}
            onPickIndex={(index) => {
              setCustomPickerOpen(false);
              onColorChange(groupId, index === 0 ? null : index);
            }}
            onToggleCustom={() => setCustomPickerOpen((open) => !open)}
          />
        </div>

        {customPickerOpen ? (
          <ColorPickerPopover
            value={customColor ?? normalizeHex(currentColor)}
            onChange={(color) => onCustomColorChange(groupId, color)}
          />
        ) : null}

        <div className="mb-2 px-0.5">
          <p className="mb-1 text-[11px] text-content/50">Mascot</p>
          <div className="flex items-center justify-between gap-1">
            {PROJECT_MASCOTS.map((mascot) => (
              <MascotSwatch
                key={mascot.name}
                title={mascot.name}
                selected={shownMascot === mascot.name}
                onPick={() => onMascotChange(groupId, mascot.name)}
              >
                <ProjectMascot
                  project={groupId}
                  name={mascot.name}
                  className="size-3 text-content/75"
                />
              </MascotSwatch>
            ))}
          </div>
        </div>

        {showActions ? (
          <>
            <div className="my-1 h-px bg-content/10" />

            {ITEMS.slice(0, 2).map((item) => (
              <MenuRow
                key={item.id}
                item={item}
                onHover={() => setSubmenu(null)}
                onPick={() => onPick(item.id as TabGroupMenuAction)}
              />
            ))}

            <div className="my-1 h-px bg-content/10" />

            {ITEMS.slice(2, 4).map((item) => (
              <MenuRow
                key={item.id}
                item={item}
                onHover={() => setSubmenu(null)}
                onPick={() => onPick(item.id as TabGroupMenuAction)}
              />
            ))}

            <div className="my-1 h-px bg-content/10" />

            {ITEMS.slice(4).map((item) => (
              <MenuRow
                key={item.id}
                item={item}
                onHover={() => setSubmenu(null)}
                onPick={() => onPick(item.id as TabGroupMenuAction)}
              />
            ))}
          </>
        ) : null}

        {extraItems && extraItems.length > 0 ? (
          <>
            <div className="my-1 h-px bg-content/10" />
            {extraItems.map((item) => (
              <Fragment key={item.id}>
                {item.sepBefore ? (
                  <div role="separator" className="my-1 h-px bg-content/10" />
                ) : null}
                <MenuRow
                  item={item}
                  expanded={submenu?.item.id === item.id}
                  onHover={(anchor) =>
                    setSubmenu(item.submenu && !item.disabled ? { item, anchor } : null)
                  }
                  onPick={(anchor) => {
                    if (item.submenu) setSubmenu({ item, anchor });
                    else pickExtra(item.id);
                  }}
                />
              </Fragment>
            ))}
          </>
        ) : null}
        {footer}
      </Popover>
      {submenu?.item.submenu ? (
        <ExplorerMenu
          anchor={submenu.anchor}
          ownerId={menuId}
          ariaLabel={submenu.item.label}
          items={submenu.item.submenu}
          onPick={pickExtra}
          onBack={closeSubmenu}
          onClose={closeSubmenu}
          onMouseEnter={cancelSubmenuClose}
          onMouseLeave={scheduleSubmenuClose}
        />
      ) : null}
    </>
  );
}

function MascotSwatch({
  title,
  selected,
  onPick,
  children,
}: {
  title: string;
  selected: boolean;
  onPick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={`Mascot ${title}`}
      aria-pressed={selected}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onPick}
      className={`grid size-5 shrink-0 place-items-center rounded-md ${
        selected
          ? "bg-selection-hover ring-1 ring-content/50"
          : "hover:bg-content/8"
      }`}
    >
      {children}
    </button>
  );
}

function MenuRow({
  item,
  onPick,
  onHover,
  expanded,
}: {
  item: MenuItem & Pick<TabGroupMenuExtraItem, "disabled" | "submenu" | "description">;
  onPick: (anchor: HTMLButtonElement) => void;
  onHover?: (anchor: HTMLButtonElement) => void;
  expanded?: boolean;
}) {
  const Icon = item.icon;
  return (
    <button
      type="button"
      role="menuitem"
      disabled={item.disabled}
      aria-haspopup={item.submenu ? "menu" : undefined}
      aria-expanded={item.submenu ? expanded : undefined}
      aria-label={item.description ? item.label : undefined}
      aria-description={item.description}
      onMouseDown={(e) => e.preventDefault()}
      onMouseEnter={(e) => onHover?.(e.currentTarget)}
      onClick={(e) => onPick(e.currentTarget)}
      onKeyDown={(e) => {
        if (item.submenu && e.key === "ArrowRight") {
          e.preventDefault();
          onPick(e.currentTarget);
        }
      }}
      className={`flex min-h-8 w-full items-center gap-2.5 rounded-lg px-2 text-left text-[13px] leading-none ${
        item.disabled
          ? "text-content/30"
          : item.danger
            ? "text-red-300/90 hover:bg-red-500/15"
            : "text-content hover:bg-content/5"
      }`}
    >
      <Icon className="size-3.5 shrink-0 text-content/55" strokeWidth={1.75} />
      <span className={`min-w-0 flex-1 leading-label ${item.description ? "py-2" : "truncate"}`}>
        {item.label}
        {item.description ? (
          <span className="mt-1 block text-[11px] leading-snug text-content/60">
            {item.description}
          </span>
        ) : null}
      </span>
      {item.submenu ? (
        <ChevronRight className="size-3.5 shrink-0 text-content/50" strokeWidth={1.75} />
      ) : null}
      {item.shortcut ? (
        <span className="shrink-0 text-[11px] text-content/40">
          {item.shortcut}
        </span>
      ) : null}
    </button>
  );
}

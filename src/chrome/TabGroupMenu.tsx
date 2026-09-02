import {
  AppWindow,
  ImagePlus,
  SquarePlus,
  Trash2,
  Ungroup,
  X,
  type IconComponent,
} from "./icons";
import {
  Fragment,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { normalizeHex } from "../lib/colorUtils";
import { clearProjectLogo, pickAndSetProjectLogo } from "../lib/projectLogos";
import { PROJECT_MASCOTS, projectMascot } from "../lib/projectMascots";
import { TAB_GROUP_COLORS } from "../lib/tabGroups";
import { ColorPickerPopover, ColorSwatchRow } from "./ColorPickerPopover";
import { Popover } from "./Popover";
import { ProjectLogoIcon } from "./ProjectLogoIcon";
import { ProjectMascot } from "./ProjectMascot";
import { MOD } from "../lib/platform";

export type TabGroupMenuAction =
  | "new-tab"
  | "new-window"
  | "close-group"
  | "ungroup"
  | "delete-group";

export type TabGroupMenuExtraItem = {
  id: string;
  label: string;
  icon: IconComponent;
  danger?: boolean;
  sepBefore?: boolean;
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
  /** When false, only name / logo / color controls are shown. */
  showActions?: boolean;
  extraItems?: TabGroupMenuExtraItem[];
  onExtraPick?: (id: string) => void;
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
  showActions = true,
  extraItems,
  onExtraPick,
}: Props) {
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
    <Popover
      anchor={{ x, y }}
      gap={0}
      width={MENU_WIDTH}
      onDismiss={onClose}
      role="menu"
      tabIndex={-1}
      aria-label="Tab group actions"
      onKeyDown={onMenuKey}
      onContextMenu={(e) => e.preventDefault()}
      className="overflow-y-auto overscroll-none p-2"
    >
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
                void clearProjectLogo(logoProject).then(onLogoChange);
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
            <MenuRow key={item.id} item={item} onPick={() => onPick(item.id as TabGroupMenuAction)} />
          ))}

          <div className="my-1 h-px bg-content/10" />

          {ITEMS.slice(2, 4).map((item) => (
            <MenuRow key={item.id} item={item} onPick={() => onPick(item.id as TabGroupMenuAction)} />
          ))}

          <div className="my-1 h-px bg-content/10" />

          {ITEMS.slice(4).map((item) => (
            <MenuRow key={item.id} item={item} onPick={() => onPick(item.id as TabGroupMenuAction)} />
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
                onPick={() => {
                  onExtraPick?.(item.id);
                  onClose();
                }}
              />
            </Fragment>
          ))}
        </>
      ) : null}
    </Popover>
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
        selected ? "bg-content/15 ring-1 ring-content/50" : "hover:bg-content/8"
      }`}
    >
      {children}
    </button>
  );
}

function MenuRow({
  item,
  onPick,
}: {
  item: MenuItem;
  onPick: () => void;
}) {
  const Icon = item.icon;
  return (
    <button
      type="button"
      role="menuitem"
      onMouseDown={(e) => e.preventDefault()}
      onClick={onPick}
      className={`flex h-8 w-full items-center gap-2.5 rounded-lg px-2 text-left text-[13px] leading-none ${
        item.danger
          ? "text-red-300/90 hover:bg-red-500/15"
          : "text-content hover:bg-content/5"
      }`}
    >
      <Icon className="size-3.5 shrink-0 text-content/55" strokeWidth={1.75} />
      <span className="min-w-0 flex-1 truncate">{item.label}</span>
      {item.shortcut ? (
        <span className="shrink-0 text-[11px] text-content/40">
          {item.shortcut}
        </span>
      ) : null}
    </button>
  );
}

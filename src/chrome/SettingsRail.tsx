import {
  Archive,
  ArrowLeft,
  Bot,
  ExternalLink,
  Keyboard,
  Palette,
  SlidersHorizontal,
  type IconComponent,
} from "./icons";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import { isCompanionClient } from "../lib/transport";
import {
  visibleSettingsSections,
  type SettingsSectionId,
} from "../lib/settings";

const SECTION_ICONS: Record<SettingsSectionId, IconComponent> = {
  general: SlidersHorizontal,
  appearance: Palette,
  keybindings: Keyboard,
  providers: Bot,
  archive: Archive,
  companion: ExternalLink,
};

type Props = {
  section: SettingsSectionId;
  onSelect: (section: SettingsSectionId) => void;
  onClose: () => void;
};

/** Body of the project rail while settings are open. */
export function SettingsNav({ section, onSelect, onClose }: Props) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  // iPad / companion: keyboard shortcuts don't apply, so the keybindings
  // section stays hidden even after disconnect (`isRemote()` is then false).
  const visible = visibleSettingsSections(isCompanionClient());

  return (
    <>
      <div
        ref={lockOverscroll}
        aria-label="Settings"
        className="flex min-h-0 flex-1 flex-col gap-px overflow-y-auto overscroll-none px-2 pb-2"
      >
        {visible.map((item) => (
          <NavRow
            key={item.id}
            label={item.label}
            icon={SECTION_ICONS[item.id]}
            active={item.id === section}
            onClick={() => onSelect(item.id)}
          />
        ))}
      </div>
      <div className="flex shrink-0 flex-col gap-px p-2">
        <NavRow label="Back" icon={ArrowLeft} onClick={onClose} />
      </div>
    </>
  );
}

function NavRow({
  label,
  icon: Icon,
  active = false,
  onClick,
}: {
  label: string;
  icon: IconComponent;
  active?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? "true" : undefined}
      className={`flex w-full items-center gap-2 rounded-md px-2 py-2 text-left ${
        active
          ? "bg-content/10 text-content"
          : "text-content/50 hover:bg-content/5 hover:text-content"
      }`}
    >
      <Icon className="size-4 shrink-0 opacity-70" strokeWidth={1.75} />
      <span className="min-w-0 flex-1 truncate text-sm font-medium leading-tight">
        {label}
      </span>
    </button>
  );
}

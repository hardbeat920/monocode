import {
  Archive,
  ArrowLeft,
  Bot,
  Inbox,
  Keyboard,
  Palette,
  SlidersHorizontal,
  Sparkles,
  type IconComponent,
} from "./icons";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import { SETTINGS_SECTIONS, type SettingsSectionId } from "../lib/settings";
import { RailAction } from "./RailAction";

const SECTION_ICONS: Record<SettingsSectionId, IconComponent> = {
  general: SlidersHorizontal,
  appearance: Palette,
  keybindings: Keyboard,
  providers: Bot,
  inbox: Inbox,
  skills: Sparkles,
  archive: Archive,
};

type Props = {
  section: SettingsSectionId;
  onSelect: (section: SettingsSectionId) => void;
  onClose: () => void;
};

/** Body of the project rail while settings are open. */
export function SettingsNav({ section, onSelect, onClose }: Props) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();

  return (
    <>
      <div
        ref={lockOverscroll}
        aria-label="Settings"
        className="flex min-h-0 flex-1 flex-col gap-px overflow-y-auto overscroll-none px-2 pb-2"
      >
        {SETTINGS_SECTIONS.map((item) => (
          <RailAction
            key={item.id}
            label={item.label}
            icon={SECTION_ICONS[item.id]}
            active={item.id === section}
            onClick={() => onSelect(item.id)}
          />
        ))}
      </div>
      <div className="flex shrink-0 flex-col gap-px p-2">
        <RailAction label="Back" icon={ArrowLeft} onClick={onClose} />
      </div>
    </>
  );
}

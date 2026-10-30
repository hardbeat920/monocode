import Add01Icon from "@hugeicons/core-free-icons/Add01Icon";
import ArrowDown01Icon from "@hugeicons/core-free-icons/ArrowDown01Icon";
import ArrowRight01Icon from "@hugeicons/core-free-icons/ArrowRight01Icon";
import ArrowUp02Icon from "@hugeicons/core-free-icons/ArrowUp02Icon";
import BotIcon from "@hugeicons/core-free-icons/BotIcon";
import BubbleChatIcon from "@hugeicons/core-free-icons/BubbleChatIcon";
import Cancel01Icon from "@hugeicons/core-free-icons/Cancel01Icon";
import ChartBreakoutSquareIcon from "@hugeicons/core-free-icons/ChartBreakoutSquareIcon";
import ClipboardPasteIcon from "@hugeicons/core-free-icons/ClipboardPasteIcon";
import ComputerIcon from "@hugeicons/core-free-icons/ComputerIcon";
import ComputerRemoveIcon from "@hugeicons/core-free-icons/ComputerRemoveIcon";
import ComputerTerminal01Icon from "@hugeicons/core-free-icons/ComputerTerminal01Icon";
import Copy01Icon from "@hugeicons/core-free-icons/Copy01Icon";
import DarkModeIcon from "@hugeicons/core-free-icons/DarkModeIcon";
import Delete02Icon from "@hugeicons/core-free-icons/Delete02Icon";
import File01Icon from "@hugeicons/core-free-icons/File01Icon";
import FilterHorizontalIcon from "@hugeicons/core-free-icons/FilterHorizontalIcon";
import FileScriptIcon from "@hugeicons/core-free-icons/FileScriptIcon";
import FlashIcon from "@hugeicons/core-free-icons/FlashIcon";
import Folder01Icon from "@hugeicons/core-free-icons/Folder01Icon";
import GitBranchIcon from "@hugeicons/core-free-icons/GitBranchIcon";
import Image01Icon from "@hugeicons/core-free-icons/Image01Icon";
import InboxIcon from "@hugeicons/core-free-icons/InboxIcon";
import KeyboardOffIcon from "@hugeicons/core-free-icons/KeyboardOffIcon";
import LockIcon from "@hugeicons/core-free-icons/LockIcon";
import LeftToRightListBulletIcon from "@hugeicons/core-free-icons/LeftToRightListBulletIcon";
import Moon02Icon from "@hugeicons/core-free-icons/Moon02Icon";
import MinusSignIcon from "@hugeicons/core-free-icons/MinusSignIcon";
import Menu01Icon from "@hugeicons/core-free-icons/Menu01Icon";
import Mic01Icon from "@hugeicons/core-free-icons/Mic01Icon";
import Note01Icon from "@hugeicons/core-free-icons/Note01Icon";
import Notification01Icon from "@hugeicons/core-free-icons/Notification01Icon";
import PencilEdit01Icon from "@hugeicons/core-free-icons/PencilEdit01Icon";
import PencilEdit02Icon from "@hugeicons/core-free-icons/PencilEdit02Icon";
import PlayIcon from "@hugeicons/core-free-icons/PlayIcon";
import Robot01Icon from "@hugeicons/core-free-icons/Robot01Icon";
import Refresh01Icon from "@hugeicons/core-free-icons/Refresh01Icon";
import Search01Icon from "@hugeicons/core-free-icons/Search01Icon";
import Settings01Icon from "@hugeicons/core-free-icons/Settings01Icon";
import Shield01Icon from "@hugeicons/core-free-icons/Shield01Icon";
import Sun03Icon from "@hugeicons/core-free-icons/Sun03Icon";
import SourceCodeIcon from "@hugeicons/core-free-icons/SourceCodeIcon";
import SparklesIcon from "@hugeicons/core-free-icons/SparklesIcon";
import TextWrapIcon from "@hugeicons/core-free-icons/TextWrapIcon";
import Tick02Icon from "@hugeicons/core-free-icons/Tick02Icon";
import UnfoldMoreIcon from "@hugeicons/core-free-icons/UnfoldMoreIcon";
import ViewIcon from "@hugeicons/core-free-icons/ViewIcon";
import Wrench01Icon from "@hugeicons/core-free-icons/Wrench01Icon";
import { HugeiconsIcon } from "@hugeicons/react-native";
import type { ColorValue } from "react-native";

import { useColors } from "./theme";

// Hugeicons, the same set as the desktop chrome (src/shared/ui/icons.tsx).
// Deep imports keep the full catalog out of the bundle.
const icons = {
  menu: Menu01Icon,
  search: Search01Icon,
  close: Cancel01Icon,
  add: Add01Icon,
  compose: PencilEdit02Icon,
  chat: BubbleChatIcon,
  settings: Settings01Icon,
  folder: Folder01Icon,
  file: File01Icon,
  send: ArrowUp02Icon,
  mic: Mic01Icon,
  chevronDown: ArrowDown01Icon,
  chevronUpDown: UnfoldMoreIcon,
  chevronRight: ArrowRight01Icon,
  branch: GitBranchIcon,
  computer: ComputerIcon,
  computerOff: ComputerRemoveIcon,
  sparkle: SparklesIcon,
  terminal: ComputerTerminal01Icon,
  list: LeftToRightListBulletIcon,
  explorer: FileScriptIcon,
  source: SourceCodeIcon,
  preview: ViewIcon,
  wrap: TextWrapIcon,
  edit: PencilEdit01Icon,
  robot: Robot01Icon,
  wrench: Wrench01Icon,
  minus: MinusSignIcon,
  copy: Copy01Icon,
  paste: ClipboardPasteIcon,
  play: PlayIcon,
  keyboardOff: KeyboardOffIcon,
  trash: Delete02Icon,
  check: Tick02Icon,
  refresh: Refresh01Icon,
  metrics: ChartBreakoutSquareIcon,
  inbox: InboxIcon,
  image: Image01Icon,
  notifications: Notification01Icon,
  note: Note01Icon,
  automation: FlashIcon,
  bot: BotIcon,
  lock: LockIcon,
  shield: Shield01Icon,
  filter: FilterHorizontalIcon,
  appearance: DarkModeIcon,
  sun: Sun03Icon,
  moon: Moon02Icon,
};

export type IconName = keyof typeof icons;

export function Icon({
  name,
  size = 20,
  color: givenColor,
  strokeWidth = 1.75,
}: {
  name: IconName;
  size?: number;
  color?: ColorValue;
  strokeWidth?: number;
}) {
  const colors = useColors();
  const color = givenColor ?? colors.content;
  return <HugeiconsIcon icon={icons[name]} size={size} color={color} strokeWidth={strokeWidth} />;
}

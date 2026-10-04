//! The chrome icon set. Port of src/shared/ui/icons.tsx: each name maps to the
//! Hugeicons glyph the React wrapper used, exported to
//! `assets/icons/<kebab-name>.svg` with the wrapper's 1.75 stroke width.
//! Also the provider logos from src/assets/providers and
//! src/features/sessions/ui/HarnessIcon.tsx.

use gpui::{
    AnyElement, App, Hsla, IntoElement, ParentElement, RenderOnce, Styled, Svg, Window, div, img,
    relative, svg,
};

use crate::u;

/// Every chrome icon. Variant names match the React exports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconName {
    /// Hugeicons `AlertCircleIcon`.
    AlertCircle,
    /// Hugeicons `AppWindowIcon`.
    AppWindow,
    /// Hugeicons `Archive02Icon`.
    Archive,
    /// Hugeicons `CircleArrowDown01Icon`.
    ArrowDownCircle,
    /// Hugeicons `ArrowLeft01Icon`.
    ArrowLeft,
    /// Hugeicons `ArrowUp01Icon`.
    ArrowUp,
    /// Hugeicons `BotIcon`.
    Bot,
    /// Hugeicons `AiIdeaIcon`.
    AiIdea,
    /// Hugeicons `CaseSensitiveIcon`.
    CaseSensitive,
    /// Hugeicons `Chatting01Icon`.
    Chatting,
    /// Hugeicons `Tick02Icon`.
    Check,
    /// Hugeicons `TickDouble02Icon`.
    CheckCheck,
    /// Hugeicons `CheckmarkCircle02Icon`.
    CheckCircle,
    /// Hugeicons `ArrowDown01Icon`.
    ChevronDown,
    /// Hugeicons `ArrowLeft01Icon`.
    ChevronLeft,
    /// Hugeicons `ArrowRight01Icon`.
    ChevronRight,
    /// Hugeicons `ArrowTurnForwardIcon`.
    CornerDownRight,
    /// Hugeicons `ArrowUp01Icon`.
    ChevronUp,
    /// Hugeicons `UnfoldMoreIcon`.
    ChevronsUpDown,
    /// Hugeicons `AlertCircleIcon`.
    CircleAlert,
    /// Hugeicons `CircleDashedIcon`.
    CircleDashed,
    /// Hugeicons `CircleDotIcon`.
    CircleDot,
    /// Hugeicons `HelpCircleIcon`.
    CircleHelp,
    /// Hugeicons `CancelCircleIcon`.
    CircleX,
    /// Hugeicons `CloudUploadIcon`.
    CloudUpload,
    /// Hugeicons `Clock01Icon`.
    Clock,
    /// Hugeicons `Copy01Icon`.
    Copy,
    /// Hugeicons `CursorMagicSelection04Icon`.
    CursorMagicSelection,
    /// Hugeicons `DashboardSquare01Icon`.
    DashboardSquare,
    /// Hugeicons `LinkSquare02Icon`.
    ExternalLink,
    /// Hugeicons `File01Icon`.
    File,
    /// Hugeicons `FileDiffIcon`.
    FileDiff,
    /// Hugeicons `FileAddIcon`.
    FilePlus,
    /// Hugeicons `FilePlusCornerIcon`.
    FilePlusCorner,
    /// Hugeicons `FileScriptIcon`.
    FileScript,
    /// Hugeicons `FoldVerticalIcon`.
    FoldVertical,
    /// Hugeicons `Folder01Icon`.
    Folder,
    /// Hugeicons `FolderOpenIcon`.
    FolderOpen,
    /// Hugeicons `ViewIcon`.
    Eye,
    /// Hugeicons `FolderAddIcon`.
    FolderPlus,
    /// Hugeicons `FolderTreeIcon`.
    FolderTree,
    /// Hugeicons `GaugeIcon`.
    Gauge,
    /// Hugeicons `ChartBreakoutSquareIcon`.
    ChartBreakoutSquare,
    /// Hugeicons `GitBranchIcon`.
    GitBranch,
    /// Hugeicons `GitCompareIcon`.
    GitCompare,
    /// Hugeicons `GitMergeIcon`.
    GitMerge,
    /// Hugeicons `GitPullRequestIcon`.
    GitPullRequest,
    /// Hugeicons `GitPullRequestClosedIcon`.
    GitPullRequestClosed,
    /// Hugeicons `GitPullRequestDraftIcon`.
    GitPullRequestDraft,
    /// Hugeicons `DragDropVerticalIcon`.
    GripVertical,
    /// Hugeicons `GlobeIcon`.
    Globe,
    /// Hugeicons `InternetIcon`.
    Internet,
    /// Hugeicons `ImageAdd01Icon`.
    ImagePlus,
    /// Hugeicons `InboxIcon`.
    Inbox,
    /// Hugeicons `NotificationOff01Icon`.
    BellOff,
    /// Hugeicons `KeyboardIcon`.
    Keyboard,
    /// Hugeicons `LeftToRightListBulletIcon`.
    ListBullet,
    /// Hugeicons `ListEndIcon`.
    ListEnd,
    /// Hugeicons `FilterIcon`.
    ListFilter,
    /// Hugeicons `Loading03Icon`.
    Loader,
    /// Hugeicons `Loading03Icon`.
    LoaderCircle,
    /// Hugeicons `SquareLock02Icon`.
    Lock,
    /// Hugeicons `ArrowExpand01Icon`.
    Maximize2,
    /// Hugeicons `MessageMultiple01Icon`.
    MessageMultiple,
    /// Hugeicons `Comment01Icon`.
    MessageSquare,
    /// Hugeicons `CommentAdd01Icon`.
    MessageSquarePlus,
    /// Hugeicons `MinusSignIcon`.
    Minus,
    /// Hugeicons `MoreHorizontalIcon`.
    MoreHorizontal,
    /// Hugeicons `PaintBoardIcon`.
    Palette,
    /// Hugeicons `PauseIcon`.
    Pause,
    /// Hugeicons `LayoutBottomIcon`.
    PanelBottom,
    /// Hugeicons `LayoutAlignRightIcon`.
    PanelLeft,
    /// Hugeicons `SidebarRight01Icon`.
    PanelRight,
    /// Hugeicons `LayoutTopIcon`.
    PanelTop,
    /// Hugeicons `PencilEdit01Icon`.
    PenLine,
    /// Hugeicons `PencilEdit02Icon`.
    Pencil,
    /// Hugeicons `PinIcon`.
    Pin,
    /// Hugeicons `PinOffIcon`.
    PinOff,
    /// Hugeicons `PlayIcon`.
    Play,
    /// Hugeicons `ColorPickerIcon`.
    Pipette,
    /// Hugeicons `Add01Icon`.
    Plus,
    /// Hugeicons `Refresh01Icon`.
    RefreshCw,
    /// Hugeicons `RegexIcon`.
    Regex,
    /// Hugeicons `ReplaceIcon`.
    Replace,
    /// Hugeicons `RotateCcwIcon`.
    RotateCcw,
    /// Hugeicons `Search01Icon`.
    Search,
    /// Hugeicons `Settings01Icon`.
    Settings,
    /// Hugeicons `Share02Icon`.
    Share,
    /// Hugeicons `ShieldAlertIcon`.
    Shield,
    /// Hugeicons `PreferenceHorizontalIcon`.
    SlidersHorizontal,
    /// Hugeicons `SparklesIcon`.
    Sparkles,
    /// Hugeicons `SquareIcon`.
    Square,
    /// Hugeicons `AddSquareIcon`.
    SquarePlus,
    /// Hugeicons `StarIcon`.
    Star,
    /// Hugeicons `Note01Icon`.
    StickyNote,
    /// Hugeicons `ComputerTerminal01Icon`.
    Terminal,
    /// Hugeicons `TextQuoteIcon`.
    TextQuote,
    /// Hugeicons `Delete02Icon`.
    Trash2,
    /// Hugeicons `UndoIcon`.
    Undo2,
    /// Hugeicons `UnfoldVerticalIcon`.
    UnfoldVertical,
    /// Hugeicons `UngroupItemsIcon`.
    Ungroup,
    /// Hugeicons `MagicWand01Icon`.
    WandSparkles,
    /// Hugeicons `WholeWordIcon`.
    WholeWord,
    /// Hugeicons `Wrench01Icon`.
    Wrench,
    /// Hugeicons `Cancel01Icon`.
    X,
    /// Hugeicons `FlashIcon`.
    Zap,
}

impl IconName {
    /// Every icon, in the order icons.tsx exports them.
    pub const ALL: &'static [IconName] = &[
        Self::AlertCircle,
        Self::AppWindow,
        Self::Archive,
        Self::ArrowDownCircle,
        Self::ArrowLeft,
        Self::ArrowUp,
        Self::Bot,
        Self::AiIdea,
        Self::CaseSensitive,
        Self::Chatting,
        Self::Check,
        Self::CheckCheck,
        Self::CheckCircle,
        Self::ChevronDown,
        Self::ChevronLeft,
        Self::ChevronRight,
        Self::CornerDownRight,
        Self::ChevronUp,
        Self::ChevronsUpDown,
        Self::CircleAlert,
        Self::CircleDashed,
        Self::CircleDot,
        Self::CircleHelp,
        Self::CircleX,
        Self::CloudUpload,
        Self::Clock,
        Self::Copy,
        Self::CursorMagicSelection,
        Self::DashboardSquare,
        Self::ExternalLink,
        Self::File,
        Self::FileDiff,
        Self::FilePlus,
        Self::FilePlusCorner,
        Self::FileScript,
        Self::FoldVertical,
        Self::Folder,
        Self::FolderOpen,
        Self::Eye,
        Self::FolderPlus,
        Self::FolderTree,
        Self::Gauge,
        Self::ChartBreakoutSquare,
        Self::GitBranch,
        Self::GitCompare,
        Self::GitMerge,
        Self::GitPullRequest,
        Self::GitPullRequestClosed,
        Self::GitPullRequestDraft,
        Self::GripVertical,
        Self::Globe,
        Self::Internet,
        Self::ImagePlus,
        Self::Inbox,
        Self::BellOff,
        Self::Keyboard,
        Self::ListBullet,
        Self::ListEnd,
        Self::ListFilter,
        Self::Loader,
        Self::LoaderCircle,
        Self::Lock,
        Self::Maximize2,
        Self::MessageMultiple,
        Self::MessageSquare,
        Self::MessageSquarePlus,
        Self::Minus,
        Self::MoreHorizontal,
        Self::Palette,
        Self::Pause,
        Self::PanelBottom,
        Self::PanelLeft,
        Self::PanelRight,
        Self::PanelTop,
        Self::PenLine,
        Self::Pencil,
        Self::Pin,
        Self::PinOff,
        Self::Play,
        Self::Pipette,
        Self::Plus,
        Self::RefreshCw,
        Self::Regex,
        Self::Replace,
        Self::RotateCcw,
        Self::Search,
        Self::Settings,
        Self::Share,
        Self::Shield,
        Self::SlidersHorizontal,
        Self::Sparkles,
        Self::Square,
        Self::SquarePlus,
        Self::Star,
        Self::StickyNote,
        Self::Terminal,
        Self::TextQuote,
        Self::Trash2,
        Self::Undo2,
        Self::UnfoldVertical,
        Self::Ungroup,
        Self::WandSparkles,
        Self::WholeWord,
        Self::Wrench,
        Self::X,
        Self::Zap,
    ];

    /// The asset path served by [`crate::Assets`].
    pub fn path(self) -> &'static str {
        match self {
            Self::AlertCircle => "monocode/icons/alert-circle.svg",
            Self::AppWindow => "monocode/icons/app-window.svg",
            Self::Archive => "monocode/icons/archive.svg",
            Self::ArrowDownCircle => "monocode/icons/arrow-down-circle.svg",
            Self::ArrowLeft => "monocode/icons/arrow-left.svg",
            Self::ArrowUp => "monocode/icons/arrow-up.svg",
            Self::Bot => "monocode/icons/bot.svg",
            Self::AiIdea => "monocode/icons/ai-idea.svg",
            Self::CaseSensitive => "monocode/icons/case-sensitive.svg",
            Self::Chatting => "monocode/icons/chatting.svg",
            Self::Check => "monocode/icons/check.svg",
            Self::CheckCheck => "monocode/icons/check-check.svg",
            Self::CheckCircle => "monocode/icons/check-circle.svg",
            Self::ChevronDown => "monocode/icons/chevron-down.svg",
            Self::ChevronLeft => "monocode/icons/chevron-left.svg",
            Self::ChevronRight => "monocode/icons/chevron-right.svg",
            Self::CornerDownRight => "monocode/icons/corner-down-right.svg",
            Self::ChevronUp => "monocode/icons/chevron-up.svg",
            Self::ChevronsUpDown => "monocode/icons/chevrons-up-down.svg",
            Self::CircleAlert => "monocode/icons/circle-alert.svg",
            Self::CircleDashed => "monocode/icons/circle-dashed.svg",
            Self::CircleDot => "monocode/icons/circle-dot.svg",
            Self::CircleHelp => "monocode/icons/circle-help.svg",
            Self::CircleX => "monocode/icons/circle-x.svg",
            Self::CloudUpload => "monocode/icons/cloud-upload.svg",
            Self::Clock => "monocode/icons/clock.svg",
            Self::Copy => "monocode/icons/copy.svg",
            Self::CursorMagicSelection => "monocode/icons/cursor-magic-selection.svg",
            Self::DashboardSquare => "monocode/icons/dashboard-square.svg",
            Self::ExternalLink => "monocode/icons/external-link.svg",
            Self::File => "monocode/icons/file.svg",
            Self::FileDiff => "monocode/icons/file-diff.svg",
            Self::FilePlus => "monocode/icons/file-plus.svg",
            Self::FilePlusCorner => "monocode/icons/file-plus-corner.svg",
            Self::FileScript => "monocode/icons/file-script.svg",
            Self::FoldVertical => "monocode/icons/fold-vertical.svg",
            Self::Folder => "monocode/icons/folder.svg",
            Self::FolderOpen => "monocode/icons/folder-open.svg",
            Self::Eye => "monocode/icons/eye.svg",
            Self::FolderPlus => "monocode/icons/folder-plus.svg",
            Self::FolderTree => "monocode/icons/folder-tree.svg",
            Self::Gauge => "monocode/icons/gauge.svg",
            Self::ChartBreakoutSquare => "monocode/icons/chart-breakout-square.svg",
            Self::GitBranch => "monocode/icons/git-branch.svg",
            Self::GitCompare => "monocode/icons/git-compare.svg",
            Self::GitMerge => "monocode/icons/git-merge.svg",
            Self::GitPullRequest => "monocode/icons/git-pull-request.svg",
            Self::GitPullRequestClosed => "monocode/icons/git-pull-request-closed.svg",
            Self::GitPullRequestDraft => "monocode/icons/git-pull-request-draft.svg",
            Self::GripVertical => "monocode/icons/grip-vertical.svg",
            Self::Globe => "monocode/icons/globe.svg",
            Self::Internet => "monocode/icons/internet.svg",
            Self::ImagePlus => "monocode/icons/image-plus.svg",
            Self::Inbox => "monocode/icons/inbox.svg",
            Self::BellOff => "monocode/icons/bell-off.svg",
            Self::Keyboard => "monocode/icons/keyboard.svg",
            Self::ListBullet => "monocode/icons/list-bullet.svg",
            Self::ListEnd => "monocode/icons/list-end.svg",
            Self::ListFilter => "monocode/icons/list-filter.svg",
            Self::Loader => "monocode/icons/loader.svg",
            Self::LoaderCircle => "monocode/icons/loader-circle.svg",
            Self::Lock => "monocode/icons/lock.svg",
            Self::Maximize2 => "monocode/icons/maximize-2.svg",
            Self::MessageMultiple => "monocode/icons/message-multiple.svg",
            Self::MessageSquare => "monocode/icons/message-square.svg",
            Self::MessageSquarePlus => "monocode/icons/message-square-plus.svg",
            Self::Minus => "monocode/icons/minus.svg",
            Self::MoreHorizontal => "monocode/icons/more-horizontal.svg",
            Self::Palette => "monocode/icons/palette.svg",
            Self::Pause => "monocode/icons/pause.svg",
            Self::PanelBottom => "monocode/icons/panel-bottom.svg",
            Self::PanelLeft => "monocode/icons/panel-left.svg",
            Self::PanelRight => "monocode/icons/panel-right.svg",
            Self::PanelTop => "monocode/icons/panel-top.svg",
            Self::PenLine => "monocode/icons/pen-line.svg",
            Self::Pencil => "monocode/icons/pencil.svg",
            Self::Pin => "monocode/icons/pin.svg",
            Self::PinOff => "monocode/icons/pin-off.svg",
            Self::Play => "monocode/icons/play.svg",
            Self::Pipette => "monocode/icons/pipette.svg",
            Self::Plus => "monocode/icons/plus.svg",
            Self::RefreshCw => "monocode/icons/refresh-cw.svg",
            Self::Regex => "monocode/icons/regex.svg",
            Self::Replace => "monocode/icons/replace.svg",
            Self::RotateCcw => "monocode/icons/rotate-ccw.svg",
            Self::Search => "monocode/icons/search.svg",
            Self::Settings => "monocode/icons/settings.svg",
            Self::Share => "monocode/icons/share.svg",
            Self::Shield => "monocode/icons/shield.svg",
            Self::SlidersHorizontal => "monocode/icons/sliders-horizontal.svg",
            Self::Sparkles => "monocode/icons/sparkles.svg",
            Self::Square => "monocode/icons/square.svg",
            Self::SquarePlus => "monocode/icons/square-plus.svg",
            Self::Star => "monocode/icons/star.svg",
            Self::StickyNote => "monocode/icons/sticky-note.svg",
            Self::Terminal => "monocode/icons/terminal.svg",
            Self::TextQuote => "monocode/icons/text-quote.svg",
            Self::Trash2 => "monocode/icons/trash-2.svg",
            Self::Undo2 => "monocode/icons/undo-2.svg",
            Self::UnfoldVertical => "monocode/icons/unfold-vertical.svg",
            Self::Ungroup => "monocode/icons/ungroup.svg",
            Self::WandSparkles => "monocode/icons/wand-sparkles.svg",
            Self::WholeWord => "monocode/icons/whole-word.svg",
            Self::Wrench => "monocode/icons/wrench.svg",
            Self::X => "monocode/icons/x.svg",
            Self::Zap => "monocode/icons/zap.svg",
        }
    }

    /// The React export name.
    pub fn name(self) -> &'static str {
        match self {
            Self::AlertCircle => "AlertCircle",
            Self::AppWindow => "AppWindow",
            Self::Archive => "Archive",
            Self::ArrowDownCircle => "ArrowDownCircle",
            Self::ArrowLeft => "ArrowLeft",
            Self::ArrowUp => "ArrowUp",
            Self::Bot => "Bot",
            Self::AiIdea => "AiIdea",
            Self::CaseSensitive => "CaseSensitive",
            Self::Chatting => "Chatting",
            Self::Check => "Check",
            Self::CheckCheck => "CheckCheck",
            Self::CheckCircle => "CheckCircle",
            Self::ChevronDown => "ChevronDown",
            Self::ChevronLeft => "ChevronLeft",
            Self::ChevronRight => "ChevronRight",
            Self::CornerDownRight => "CornerDownRight",
            Self::ChevronUp => "ChevronUp",
            Self::ChevronsUpDown => "ChevronsUpDown",
            Self::CircleAlert => "CircleAlert",
            Self::CircleDashed => "CircleDashed",
            Self::CircleDot => "CircleDot",
            Self::CircleHelp => "CircleHelp",
            Self::CircleX => "CircleX",
            Self::CloudUpload => "CloudUpload",
            Self::Clock => "Clock",
            Self::Copy => "Copy",
            Self::CursorMagicSelection => "CursorMagicSelection",
            Self::DashboardSquare => "DashboardSquare",
            Self::ExternalLink => "ExternalLink",
            Self::File => "File",
            Self::FileDiff => "FileDiff",
            Self::FilePlus => "FilePlus",
            Self::FilePlusCorner => "FilePlusCorner",
            Self::FileScript => "FileScript",
            Self::FoldVertical => "FoldVertical",
            Self::Folder => "Folder",
            Self::FolderOpen => "FolderOpen",
            Self::Eye => "Eye",
            Self::FolderPlus => "FolderPlus",
            Self::FolderTree => "FolderTree",
            Self::Gauge => "Gauge",
            Self::ChartBreakoutSquare => "ChartBreakoutSquare",
            Self::GitBranch => "GitBranch",
            Self::GitCompare => "GitCompare",
            Self::GitMerge => "GitMerge",
            Self::GitPullRequest => "GitPullRequest",
            Self::GitPullRequestClosed => "GitPullRequestClosed",
            Self::GitPullRequestDraft => "GitPullRequestDraft",
            Self::GripVertical => "GripVertical",
            Self::Globe => "Globe",
            Self::Internet => "Internet",
            Self::ImagePlus => "ImagePlus",
            Self::Inbox => "Inbox",
            Self::BellOff => "BellOff",
            Self::Keyboard => "Keyboard",
            Self::ListBullet => "ListBullet",
            Self::ListEnd => "ListEnd",
            Self::ListFilter => "ListFilter",
            Self::Loader => "Loader",
            Self::LoaderCircle => "LoaderCircle",
            Self::Lock => "Lock",
            Self::Maximize2 => "Maximize2",
            Self::MessageMultiple => "MessageMultiple",
            Self::MessageSquare => "MessageSquare",
            Self::MessageSquarePlus => "MessageSquarePlus",
            Self::Minus => "Minus",
            Self::MoreHorizontal => "MoreHorizontal",
            Self::Palette => "Palette",
            Self::Pause => "Pause",
            Self::PanelBottom => "PanelBottom",
            Self::PanelLeft => "PanelLeft",
            Self::PanelRight => "PanelRight",
            Self::PanelTop => "PanelTop",
            Self::PenLine => "PenLine",
            Self::Pencil => "Pencil",
            Self::Pin => "Pin",
            Self::PinOff => "PinOff",
            Self::Play => "Play",
            Self::Pipette => "Pipette",
            Self::Plus => "Plus",
            Self::RefreshCw => "RefreshCw",
            Self::Regex => "Regex",
            Self::Replace => "Replace",
            Self::RotateCcw => "RotateCcw",
            Self::Search => "Search",
            Self::Settings => "Settings",
            Self::Share => "Share",
            Self::Shield => "Shield",
            Self::SlidersHorizontal => "SlidersHorizontal",
            Self::Sparkles => "Sparkles",
            Self::Square => "Square",
            Self::SquarePlus => "SquarePlus",
            Self::Star => "Star",
            Self::StickyNote => "StickyNote",
            Self::Terminal => "Terminal",
            Self::TextQuote => "TextQuote",
            Self::Trash2 => "Trash2",
            Self::Undo2 => "Undo2",
            Self::UnfoldVertical => "UnfoldVertical",
            Self::Ungroup => "Ungroup",
            Self::WandSparkles => "WandSparkles",
            Self::WholeWord => "WholeWord",
            Self::Wrench => "Wrench",
            Self::X => "X",
            Self::Zap => "Zap",
        }
    }

    /// Looks an icon up by its React export name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|icon| icon.name() == name)
    }
}

/// An icon element, 16px by default. Set `.size(..)` and always
/// `.text_color(..)`: GPUI svgs do not inherit the parent's text color, and
/// one without a color paints nothing. Use `group_hover` for hover ink.
pub fn icon(name: IconName) -> Svg {
    svg().path(name.path()).flex_none().size(u(16.))
}

/// A provider (harness) logo. Ids match `HarnessId` in session.ts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProviderLogo {
    Claude,
    Codex,
    Cursor,
    Grok,
    Opencode,
    Pi,
    Omp,
    Fx,
    Hermes,
    Droid,
    Antigravity,
}

impl ProviderLogo {
    pub const ALL: &'static [ProviderLogo] = &[
        Self::Claude,
        Self::Codex,
        Self::Cursor,
        Self::Grok,
        Self::Opencode,
        Self::Pi,
        Self::Omp,
        Self::Fx,
        Self::Hermes,
        Self::Droid,
        Self::Antigravity,
    ];

    /// The `HarnessId` string.
    pub fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::Grok => "grok",
            Self::Opencode => "opencode",
            Self::Pi => "pi",
            Self::Omp => "omp",
            Self::Fx => "fx",
            Self::Hermes => "hermes",
            Self::Droid => "droid",
            Self::Antigravity => "antigravity",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|logo| logo.id() == id)
    }

    pub fn path(self) -> &'static str {
        match self {
            Self::Claude => "monocode/providers/claude.svg",
            Self::Codex => "monocode/providers/codex.svg",
            Self::Cursor => "monocode/providers/cursor.svg",
            Self::Grok => "monocode/providers/grok.svg",
            Self::Opencode => "monocode/providers/opencode.svg",
            Self::Pi => "monocode/providers/pi.svg",
            Self::Omp => "monocode/providers/omp.svg",
            Self::Fx => "monocode/providers/fx.svg",
            Self::Hermes => "monocode/providers/hermes.svg",
            Self::Droid => "monocode/providers/droid.svg",
            Self::Antigravity => "monocode/providers/antigravity.svg",
        }
    }

    /// `MONOCHROME_HARNESSES`: white marks drawn in the current text color so
    /// they stay visible in light mode.
    pub fn is_monochrome(self) -> bool {
        matches!(
            self,
            Self::Cursor
                | Self::Grok
                | Self::Opencode
                | Self::Pi
                | Self::Fx
                | Self::Hermes
                | Self::Droid
        )
    }

    /// HarnessIcon shrinks the hermes and droid masks inside their box.
    fn inner_scale(self) -> f32 {
        match self {
            Self::Hermes => 0.72,
            Self::Droid => 0.88,
            _ => 1.0,
        }
    }
}

/// A provider logo at `size` CSS px. Monochrome marks use `color`.
#[derive(IntoElement)]
pub struct Provider {
    logo: ProviderLogo,
    size: f32,
    color: Option<Hsla>,
}

/// `HarnessIcon`: 14px by default.
pub fn provider_logo(logo: ProviderLogo) -> Provider {
    Provider {
        logo,
        size: 14.,
        color: None,
    }
}

impl Provider {
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Provider {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let frame = div()
            .flex_none()
            .size(u(self.size))
            .flex()
            .items_center()
            .justify_center();
        let mark: AnyElement = if self.logo.is_monochrome() {
            let color = self
                .color
                .unwrap_or_else(|| crate::Theme::of(cx).colors.content);
            svg()
                .path(self.logo.path())
                .size(relative(self.logo.inner_scale()))
                .text_color(color)
                .into_any_element()
        } else {
            img(self.logo.path()).size_full().into_any_element()
        };
        frame.child(mark)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::AssetSource;

    #[test]
    fn every_icon_has_an_asset() {
        assert_eq!(IconName::ALL.len(), 106);
        for icon in IconName::ALL {
            let data = crate::Assets.load(icon.path()).unwrap();
            assert!(data.is_some(), "missing {}", icon.path());
            assert_eq!(IconName::from_name(icon.name()), Some(*icon));
        }
    }

    #[test]
    fn every_provider_has_a_logo() {
        for logo in ProviderLogo::ALL {
            assert!(crate::Assets.load(logo.path()).unwrap().is_some());
            assert_eq!(ProviderLogo::from_id(logo.id()), Some(*logo));
        }
    }
}

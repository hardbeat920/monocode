//! Shared widgets, styled from [`crate::Theme`]. Ports of the primitives in
//! src/shared/ui and the inline styles the shell repeats.

pub mod badge;
pub mod button;
pub mod diff_stat;
pub mod input;
pub mod kbd;
pub mod menu;
pub mod modal;
pub mod popover;
pub mod segmented;
pub mod spinner;
pub mod switch;
pub mod toast;
pub mod tooltip;

pub use badge::{Badge, Dot, Tag, badge, dot, tag};
pub use button::{Button, ButtonVariant, IconButton, button, icon_button};
pub use diff_stat::{DiffStat, diff_stat};
pub use input::{TextField, text_field};
pub use kbd::{Kbd, kbd};
pub use menu::{MENU_WIDTH, Menu, MenuEntry, MenuItem, context_menu, menu};
pub use modal::{Modal, ModalSize, modal};
pub use popover::{
    POPOVER_GAP, POPOVER_PADDING, PopoverFrame, PopoverSide, popover_at, popover_frame,
};
pub use segmented::{Segmented, segmented};
pub use spinner::{Spinner, spinner};
pub use switch::{Switch, switch};
pub use toast::{Toast, ToastAction, ToastKind, ToastStack, Toasts, toast_stack};
pub use tooltip::{Tooltip, tooltip, tooltip_with_shortcut};

pub fn init(cx: &mut gpui::App) {
    cx.set_global(Toasts::default());
}

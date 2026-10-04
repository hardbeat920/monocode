//! The native Connect settings section.
use crate::adapters::remote::RemoteAdapter;
use gpui::{AnyView, App, AppContext as _, Window};
use monocode_view_remote::connections::ConnectionsSettings;
use std::rc::Rc;

pub fn build(window: &mut Window, cx: &mut App) -> AnyView {
    let host = Rc::new(RemoteAdapter::new(cx));
    cx.new(|cx| ConnectionsSettings::new(host, window, cx))
        .into()
}

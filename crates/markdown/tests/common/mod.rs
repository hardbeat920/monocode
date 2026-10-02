//! Headless GPUI harness with the platform text system, so layout uses real
//! glyph metrics.

#![allow(dead_code)]

use std::sync::{Arc, Mutex, MutexGuard};

use gpui::{
    AnyWindowHandle, AppContext, Context, Entity, HeadlessAppContext, IntoElement, ParentElement,
    Render, Styled, Window, WindowHandle, div, px, size,
};
use monocode_markdown::MarkdownView;

pub struct Host {
    pub markdown: Entity<MarkdownView>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p(px(20.)).child(self.markdown.clone())
    }
}

/// AppKit and the platform setup are not safe to run from several test
/// threads at once, so headless tests take turns.
pub fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

pub fn app() -> HeadlessAppContext {
    let platform = gpui_platform::current_platform(true);
    let mut cx = HeadlessAppContext::with_platform(platform.text_system(), Arc::new(()), || None);
    cx.update(monocode_markdown::init);
    cx
}

pub fn open(
    cx: &mut HeadlessAppContext,
    width: f32,
    height: f32,
    build: impl FnOnce(&mut Context<MarkdownView>) -> MarkdownView + 'static,
) -> (WindowHandle<Host>, Entity<MarkdownView>) {
    let window = cx
        .open_window(size(px(width), px(height)), |_, cx| {
            let markdown = cx.new(build);
            cx.new(|_| Host { markdown })
        })
        .expect("open window");
    let markdown = cx
        .read_window(&window, |host, cx| host.read(cx).markdown.clone())
        .expect("host");
    (window, markdown)
}

pub fn draw(cx: &mut HeadlessAppContext, window: impl Into<AnyWindowHandle>) {
    let window = window.into();
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
    })
    .expect("draw");
    cx.run_until_parked();
}

//! Gallery views for checking monocode-ui by screenshot: every widget
//! (`--view widgets`), a modal over the shell (`--view modal`), and the icon
//! sets (`--view icons`).

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Window, div,
};
use gpui_component::input::InputState;
use monocode_ui::widgets::{
    MenuEntry, MenuItem, Toast, ToastKind, Toasts, Tooltip, badge, button, diff_stat, dot,
    icon_button, kbd, menu, modal, popover_frame, segmented, spinner, switch, tag, text_field,
    toast_stack,
};
use monocode_ui::{
    IconName, ProviderLogo, Theme, UiStyled as _, file_type_icon, folder_type_icon, icon,
    provider_logo, u,
};

fn section(title: &'static str, theme: &Theme, body: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(u(8.))
        .child(
            div()
                .text_px(theme.text.caption)
                .medium()
                .text_color(theme.content(0.45))
                .child(title),
        )
        .child(body)
}

fn row() -> gpui::Div {
    div().flex().flex_wrap().items_center().gap(u(10.))
}

pub struct WidgetsGallery {
    search: Entity<InputState>,
    filled: Entity<InputState>,
}

impl WidgetsGallery {
    pub fn build(window: &mut Window, cx: &mut App) -> gpui::AnyView {
        let view = cx.new(|cx| Self {
            search: cx.new(|cx| InputState::new(window, cx).placeholder("Search sessions")),
            filled: cx.new(|cx| InputState::new(window, cx).default_value("~/code/monocode")),
        });
        Toasts::push(
            Toast::new("Allow writes outside the worktree?")
                .kind(ToastKind::Warning)
                .status("Approval")
                .provider(ProviderLogo::Claude)
                .body("Claude wants to run `rm -rf target/agent-ui` in ~/code/monocode.")
                .meta("Claude Code")
                .action("Allow", true, |_, _| {})
                .action("Deny", false, |_, _| {}),
            cx,
        );
        view.into()
    }
}

impl Render for WidgetsGallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;

        let buttons = row()
            .child(
                button("b-primary", "Commit")
                    .primary()
                    .icon(IconName::Check),
            )
            .child(button("b-primary-off", "Commit").primary().disabled(true))
            .child(button("b-secondary", "Open in Finder").icon(IconName::FolderOpen))
            .child(button("b-ghost", "Undo All").ghost())
            .child(button("b-ghost-on", "Review").ghost().selected(true))
            .child(button("b-danger", "Delete").danger().icon(IconName::Trash2))
            .child(button("b-off", "Disabled").disabled(true));
        let icon_buttons = row()
            .child(icon_button("ib-1", IconName::PanelLeft).active(true))
            .child(icon_button("ib-2", IconName::Search))
            .child(icon_button("ib-3", IconName::Plus).accent(true))
            .child(icon_button("ib-4", IconName::ChevronLeft).disabled(true))
            .child(
                icon_button("ib-5", IconName::ListFilter)
                    .size(24.)
                    .icon_size(12.),
            )
            .child(icon_button("ib-6", IconName::X).size(20.).icon_size(12.));
        let marks = row()
            .child(badge(3))
            .child(badge(140))
            .child(dot(8.))
            .child(dot(6.).color(c.success))
            .child(tag("Development"))
            .child(kbd("⌘K"))
            .child(kbd("⌘⇧B"))
            .child(diff_stat(949, 10))
            .child(diff_stat(1478, 0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(4.))
                    .text_px(theme.text.caption)
                    .text_color(c.accent)
                    .child(spinner("gallery-spinner"))
                    .child("Working..."),
            );
        let controls = row()
            .child(switch("sw-on", true))
            .child(switch("sw-off", false))
            .child(switch("sw-disabled", true).disabled(true))
            .child(segmented("seg-scheme", ["Dark", "Light", "System"], 0))
            .child(segmented("seg-layout", ["Chat", "Full"], 1));
        let fields = row()
            .child(
                div()
                    .w(u(240.))
                    .flex()
                    .child(text_field(&self.search).icon(IconName::Search)),
            )
            .child(
                div()
                    .w(u(240.))
                    .flex()
                    .child(text_field(&self.filled).bordered()),
            );

        let menu_panel = menu(
            "gallery-menu",
            vec![
                MenuItem::new("open", "Open in New Tab")
                    .shortcut("⌘↩")
                    .into(),
                MenuItem::new("rename", "Rename")
                    .shortcut("F2")
                    .highlighted(true)
                    .into(),
                MenuItem::new("pin", "Pin").checked(true).into(),
                MenuItem::new("move", "Move to Folder").submenu().into(),
                MenuItem::new("link", "Link Issue or PR…")
                    .description("Shows its checks beside the session")
                    .into(),
                MenuEntry::Separator,
                MenuItem::new("archive", "Archive").disabled(true).into(),
                MenuItem::new("delete", "Delete").danger().into(),
            ],
        );
        let popover = popover_frame("gallery-popover").width(260.).child(
            div()
                .flex()
                .flex_col()
                .gap(u(6.))
                .p(u(10.))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .child(
                            div()
                                .text_px(theme.text.caption)
                                .semibold()
                                .text_color(theme.content(0.85))
                                .child("Subagents"),
                        )
                        .child(
                            div()
                                .text_px(theme.text.micro)
                                .tabular()
                                .text_color(theme.content(0.45))
                                .child("2/3 done"),
                        ),
                )
                .child(
                    div()
                        .text_px(theme.text.caption)
                        .text_color(theme.content(0.75))
                        .child("Popover frame: 12px radius, glass, border-content/10."),
                ),
        );
        let tooltip_sample: AnyElement = Tooltip::new("Settings")
            .shortcut("⌘,")
            .build(window, cx)
            .into_any_element();
        let floating = row()
            .items_start()
            .child(menu_panel)
            .child(popover)
            .child(tooltip_sample);

        div()
            .id("widgets-gallery")
            .size_full()
            .overflow_y_scroll()
            .bg(c.body_glass)
            .text_color(c.content)
            .font_family(theme.fonts.sans.clone())
            .line_height(gpui::relative(theme.leading.normal))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(u(22.))
                    .pt(u(56.))
                    .px(u(28.))
                    .pb(u(28.))
                    .child(
                        div()
                            .text_px(theme.text.title)
                            .medium()
                            .child("monocode-ui widgets"),
                    )
                    .child(section("Buttons", &theme, buttons))
                    .child(section("Icon buttons", &theme, icon_buttons))
                    .child(section(
                        "Badges, tags, kbd, diff stats, spinner",
                        &theme,
                        marks,
                    ))
                    .child(section("Switch and segmented control", &theme, controls))
                    .child(section("Text fields", &theme, fields))
                    .child(section("Menu, popover, tooltip", &theme, floating)),
            )
            .child(toast_stack().top_offset(52.))
    }
}

/// The shell with a modal open over it.
pub struct ModalDemo {
    shell: gpui::AnyView,
    open: bool,
}

impl ModalDemo {
    pub fn build(window: &mut Window, cx: &mut App) -> gpui::AnyView {
        let shell = crate::shell::build(crate::shell::ShellOptions::full(), window, cx);
        cx.new(|_| Self { shell, open: true }).into()
    }
}

impl Render for ModalDemo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let mut root = div().relative().size_full().child(self.shell.clone());
        if self.open {
            let weak = cx.entity().downgrade();
            root = root.child(
                modal("demo-modal", "Delete session")
                    .description("\"Benchmark arcade games\" and its transcript")
                    .size(monocode_ui::widgets::ModalSize::Sm)
                    .on_close(move |_, cx| {
                        weak.update(cx, |this, cx| {
                            this.open = false;
                            cx.notify();
                        })
                        .ok();
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(u(12.))
                            .px(u(16.))
                            .pt(u(12.))
                            .pb(u(16.))
                            .child(
                                div()
                                    .text_px(theme.text.body)
                                    .leading(theme.leading.relaxed)
                                    .text_color(theme.content(0.70))
                                    .child("The worktree at ~/.monocode/worktrees/arcade is unused. Remove it too?"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .gap(u(8.))
                                    .child(button("modal-keep", "Keep worktree"))
                                    .child(button("modal-delete", "Delete").danger()),
                            ),
                    ),
            );
        }
        root
    }
}

/// Every chrome icon, provider logo, and a sample of file-type icons.
pub struct IconsGallery;

impl IconsGallery {
    pub fn build(_: &mut Window, cx: &mut App) -> gpui::AnyView {
        cx.new(|_| Self).into()
    }
}

impl Render for IconsGallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let mut glyphs = div().flex().flex_wrap().gap(u(6.));
        for name in IconName::ALL {
            glyphs = glyphs.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(u(4.))
                    .w(u(86.))
                    .py(u(6.))
                    .rounded(u(theme.radius.md))
                    .bg(theme.content(0.04))
                    .child(icon(*name).size(u(18.)).text_color(c.content))
                    .child(
                        div()
                            .w_full()
                            .truncate()
                            .flex()
                            .justify_center()
                            .text_px(9.)
                            .text_color(theme.content(0.50))
                            .child(name.name()),
                    ),
            );
        }
        let mut logos = div().flex().flex_wrap().gap(u(14.)).items_center();
        for logo in ProviderLogo::ALL {
            logos = logos.child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(6.))
                    .child(provider_logo(*logo).size(18.))
                    .child(
                        div()
                            .text_px(theme.text.caption)
                            .text_color(theme.content(0.6))
                            .child(logo.id()),
                    ),
            );
        }
        let mut files = div().flex().flex_wrap().gap(u(14.)).items_center();
        for name in [
            "main.rs",
            "Cargo.toml",
            "package.json",
            "gridArcade.ts",
            "gridArcade.test.ts",
            "HarnessIcon.tsx",
            "index.css",
            "README.md",
            ".gitignore",
            "Dockerfile",
            "screenshot.jpg",
            "types.d.ts",
            "Makefile",
            "archive.tar.gz",
        ] {
            files = files.child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(6.))
                    .child(file_type_icon(name).size(16.))
                    .child(
                        div()
                            .text_px(theme.text.caption)
                            .text_color(theme.content(0.6))
                            .child(name),
                    ),
            );
        }
        for (name, open) in [
            ("src", false),
            ("src", true),
            ("node_modules", false),
            (".github", false),
            ("docs", true),
            ("misc", false),
        ] {
            files = files.child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(6.))
                    .child(folder_type_icon(name, open, false).size(16.))
                    .child(
                        div()
                            .text_px(theme.text.caption)
                            .text_color(theme.content(0.6))
                            .child(name),
                    ),
            );
        }
        div()
            .id("icons-gallery")
            .size_full()
            .overflow_y_scroll()
            .bg(c.body_glass)
            .text_color(c.content)
            .line_height(gpui::relative(theme.leading.normal))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(u(18.))
                    .pt(u(52.))
                    .px(u(24.))
                    .pb(u(24.))
                    .child(section(
                        "Chrome icons (Hugeicons, 1.75 stroke)",
                        &theme,
                        glyphs,
                    ))
                    .child(section("Provider logos", &theme, logos))
                    .child(section("File-type icons", &theme, files)),
            )
    }
}

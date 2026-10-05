//! Port of src/features/sessions/ui/EmptySession.tsx: a new session's
//! screen, with the question, the centered composer, and the arcade grid
//! behind them.
//!
//! The composer and the arcade are views the owner supplies. The arcade
//! (`TerminalGridBackground`) belongs to the terminal dock module.

use gpui::{
    AnyView, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};
use monocode_ui::{Theme, UiStyled as _, u};

/// What the screen shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmptySessionProps {
    pub cwd: String,
    /// The tab group label when `cwd` looks like a project
    /// (`resolveTabGroupLabel`), else `None`.
    pub project: Option<String>,
    /// A chat background image is selected, which replaces the arcade.
    pub has_chat_background: bool,
    /// `monocode.gridArcadeEnabled`.
    pub arcade_enabled: bool,
}

/// The heading: `What should we work on in <project>?`.
pub fn empty_session_title(project: Option<&str>) -> String {
    match project {
        Some(project) => format!("What should we work on in {project}?"),
        None => "What should we work on?".into(),
    }
}

/// The empty session screen.
pub struct EmptySession {
    props: EmptySessionProps,
    composer: Option<AnyView>,
    arcade: Option<AnyView>,
}

impl EmptySession {
    pub fn new(props: EmptySessionProps) -> Self {
        Self {
            props,
            composer: None,
            arcade: None,
        }
    }

    pub fn set_props(&mut self, props: EmptySessionProps, cx: &mut Context<Self>) {
        self.props = props;
        cx.notify();
    }

    /// The centered composer. Without one the screen draws only the
    /// background, as when the composer docks.
    pub fn set_composer(&mut self, composer: Option<AnyView>, cx: &mut Context<Self>) {
        self.composer = composer;
        cx.notify();
    }

    /// The arcade grid drawn behind the composer.
    pub fn set_arcade(&mut self, arcade: Option<AnyView>, cx: &mut Context<Self>) {
        self.arcade = arcade;
        cx.notify();
    }

    /// The arcade plays unless it is off or a chat background replaces it.
    pub fn shows_arcade(&self) -> bool {
        self.props.arcade_enabled && !self.props.has_chat_background
    }

    pub fn title(&self) -> String {
        empty_session_title(self.props.project.as_deref())
    }
}

impl Render for EmptySession {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let mut root = div()
            .id("empty-session")
            .relative()
            .flex()
            .size_full()
            .min_h_0()
            .overflow_y_scroll();
        if self.shows_arcade()
            && let Some(arcade) = self.arcade.clone()
        {
            root = root.child(
                div()
                    .debug_selector(|| "empty-session-arcade".into())
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .child(arcade),
            );
        }
        let Some(composer) = self.composer.clone() else {
            return root;
        };
        let title = SharedString::from(self.title());
        let mut heading = div()
            .id("empty-session-title")
            .truncate()
            .text_px(18.)
            .text_color(theme.colors.content)
            .child(title);
        if self.props.project.is_some() {
            heading = heading.tooltip(monocode_ui::widgets::tooltip(self.props.cwd.clone()));
        }
        // The same box as the docked composer (max-w-4xl, p-1.5), so the
        // input keeps its width when the first message docks it.
        root.child(
            div()
                .relative()
                .flex()
                .flex_col()
                .flex_1()
                .justify_center()
                .mx_auto()
                .w_full()
                .max_w(u(896.))
                .px(u(6.))
                .py(u(48.))
                .child(div().mb(u(16.)).px(u(10.)).child(heading))
                .child(div().w_full().child(composer)),
        )
    }
}

#[cfg(test)]
mod tests {
    //! Port of EmptySession.test.ts.

    use gpui::{AppContext as _, Context, IntoElement, Render, TestAppContext, Window, div};

    use super::*;
    use crate::panes::test_support::{draw, init};

    struct Arcade;

    impl Render for Arcade {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full()
        }
    }

    fn arcade_drawn(has_chat_background: bool, cx: &mut TestAppContext) -> bool {
        cx.update(init);
        let (_, cx) = cx.add_window_view(move |_, cx| {
            let mut screen = EmptySession::new(EmptySessionProps {
                cwd: "/work/demo".into(),
                project: Some("demo".into()),
                has_chat_background,
                arcade_enabled: true,
            });
            screen.set_arcade(Some(cx.new(|_| Arcade).into()), cx);
            screen
        });
        draw(cx);
        cx.debug_bounds("empty-session-arcade").is_some()
    }

    #[gpui::test]
    fn renders_the_arcade_when_no_chat_background_is_selected(cx: &mut TestAppContext) {
        assert!(arcade_drawn(false, cx));
    }

    #[gpui::test]
    fn does_not_render_the_arcade_over_a_selected_chat_background(cx: &mut TestAppContext) {
        assert!(!arcade_drawn(true, cx));
    }

    #[test]
    fn asks_about_the_project_when_there_is_one() {
        assert_eq!(
            empty_session_title(Some("demo")),
            "What should we work on in demo?"
        );
        assert_eq!(empty_session_title(None), "What should we work on?");
        let screen = EmptySession::new(EmptySessionProps {
            arcade_enabled: false,
            ..EmptySessionProps::default()
        });
        assert!(!screen.shows_arcade());
    }
}

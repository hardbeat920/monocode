//! A quiet rail button asking people to star MonoCode on GitHub. It only
//! appears once `gh` confirms the active account hasn't starred it yet, and
//! goes away for good when they star it or dismiss it.
use super::*;
use monocode_core::github_star::{self, StarStatus};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Loading,
    Visible,
    Starring,
    Hidden,
}

#[derive(Default)]
pub(super) struct State {
    phase: Phase,
    task: Option<gpui::Task<()>>,
}

/// Tailwind's orange-400, the colour of the desktop prompt.
fn orange(alpha: f32) -> gpui::Hsla {
    gpui::Hsla::from(gpui::rgb(0xfb923c)).opacity(alpha)
}

impl Shell {
    /// Checks once at launch, and again on each window focus when `force`,
    /// so starring in the browser hides the prompt on return.
    pub(super) fn check_github_star(&mut self, force: bool, cx: &mut Context<Self>) {
        if cfg!(test) {
            return;
        }
        let state = &mut self.github_star;
        if self.workspace.github_star_prompt_dismissed {
            state.phase = Phase::Hidden;
            return;
        }
        if state.task.is_some() || (!force && state.phase != Phase::Loading) {
            return;
        }
        let initial = state.phase == Phase::Loading;
        let work = cx
            .background_executor()
            .spawn(async move { github_star::status() });
        state.task = Some(cx.spawn(async move |this, cx| {
            let status = work.await;
            let _ = this.update(cx, |this, cx| {
                let state = &mut this.github_star;
                state.task = None;
                let next = match status {
                    StarStatus::Starred => Phase::Hidden,
                    StarStatus::NotStarred => Phase::Visible,
                    StarStatus::Unavailable if initial => Phase::Hidden,
                    StarStatus::Unavailable => state.phase,
                };
                if !this.workspace.github_star_prompt_dismissed && next != state.phase {
                    state.phase = next;
                    cx.notify();
                }
            });
        }));
    }

    fn star_github(&mut self, cx: &mut Context<Self>) {
        if self.github_star.phase == Phase::Starring {
            return;
        }
        self.github_star.phase = Phase::Starring;
        let work = cx
            .background_executor()
            .spawn(async move { github_star::star() });
        self.github_star.task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.github_star.task = None;
                if result.is_ok() {
                    this.github_star.phase = Phase::Hidden;
                } else {
                    // Starring through `gh` failed; let them do it on the web.
                    this.github_star.phase = Phase::Visible;
                    cx.open_url(github_star::REPOSITORY_URL);
                }
                cx.notify();
            });
        }));
    }

    fn dismiss_github_star(&mut self, cx: &mut Context<Self>) {
        self.workspace.github_star_prompt_dismissed = true;
        self.github_star = State {
            phase: Phase::Hidden,
            task: None,
        };
        self.persist(cx);
        cx.notify();
    }

    pub(super) fn github_star_prompt(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let busy = match self.github_star.phase {
            Phase::Visible => false,
            Phase::Starring => true,
            Phase::Loading | Phase::Hidden => return None,
        };
        let star = div()
            .id("github-star")
            .debug_selector(|| "github-star".into())
            .role(Role::Button)
            .aria_label("Star MonoCode on GitHub")
            .size_full()
            .pl_2()
            .pr(px(32.))
            .rounded_md()
            .flex()
            .items_center()
            .gap_2()
            .bg(orange(0.1))
            .text_color(orange(1.))
            .text_size(px(13.))
            .font_weight(FontWeight::MEDIUM)
            // A 16px slot lines the label up with the Settings row below.
            .child(
                div()
                    .size(px(16.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(if busy {
                        crate::pulse::looping(
                            "github-star-spin",
                            std::time::Duration::from_secs(1),
                            crate::pulse::Rate::Smooth,
                            |turn| {
                                icon("loader", 13.)
                                    .text_color(orange(1.))
                                    .with_transformation(gpui::Transformation::rotate(
                                        gpui::percentage(turn),
                                    ))
                            },
                        )
                        .into_any_element()
                    } else {
                        icon("star-filled", 13.)
                            .text_color(orange(1.))
                            .into_any_element()
                    }),
            )
            .child(if busy {
                "Starring…"
            } else {
                "Star on GitHub"
            })
            .when(busy, |d| d.opacity(0.65))
            .when(!busy, |d| {
                d.cursor_pointer()
                    .hover(|s| s.bg(orange(0.2)))
                    .on_click(cx.listener(|this, _, _, cx| this.star_github(cx)))
            });
        let dismiss = div()
            .id("github-star-dismiss")
            .debug_selector(|| "github-star-dismiss".into())
            .role(Role::Button)
            .aria_label("Don't show again")
            .absolute()
            .right(px(4.))
            .top(px(4.))
            .size(px(24.))
            .rounded_md()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(orange(0.6))
            .hover(|s| s.bg(orange(0.15)).text_color(orange(1.)))
            .child(icon("x", 12.))
            .on_click(cx.listener(|this, _, _, cx| this.dismiss_github_star(cx)));
        Some(
            div()
                .relative()
                .h(px(32.))
                .w_full()
                .child(star)
                .child(dismiss)
                .into_any(),
        )
    }
}

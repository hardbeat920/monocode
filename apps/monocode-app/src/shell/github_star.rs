//! The optional GitHub star prompt, shared across workspace windows.

#[cfg(test)]
#[path = "github_star_tests.rs"]
mod tests;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Global, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, Window, WindowId, div,
};
use monocode_app::boot::AppServices;
use monocode_engine::inbox::inbox::Inbox;
use monocode_engine::inbox::types::GithubStarStatus;
use monocode_ui::widgets::icon_button;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};
use std::collections::HashMap;

const DISMISSED: &str = "monocode.githubStarPrompt.dismissed.v1";
const URL: &str = "https://github.com/hardbeat920/monocode";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Loading,
    Visible,
    Starring,
    Hidden,
}
struct SharedPrompt(Entity<Prompt>);
impl Global for SharedPrompt {}
struct Prompt {
    kv: monocode_settings::Kv,
    generation: u64,
    phase: Phase,
    checking: bool,
    check: Option<Task<()>>,
    star: Option<Task<()>>,
    activations: HashMap<WindowId, Subscription>,
}

pub fn view(cx: &mut App) -> AnyElement {
    if AppServices::try_global(cx).is_none() {
        return div().into_any_element();
    }
    let prompt = if let Some(prompt) = cx.try_global::<SharedPrompt>() {
        prompt.0.clone()
    } else {
        let kv = AppServices::global(cx).kv.clone();
        let prompt = cx.new(|cx| Prompt::new(kv, cx));
        cx.set_global(SharedPrompt(prompt.clone()));
        prompt
    };
    prompt.into_any_element()
}

impl Prompt {
    fn new(kv: monocode_settings::Kv, cx: &mut Context<Self>) -> Self {
        let weak = cx.weak_entity();
        cx.on_window_closed(move |cx, id| {
            weak.update(cx, |this, _| {
                this.activations.remove(&id);
            })
            .ok();
        })
        .detach();
        let mut prompt = Self {
            kv,
            generation: 0,
            phase: Phase::Loading,
            checking: false,
            check: None,
            star: None,
            activations: HashMap::new(),
        };
        prompt.refresh(cx);
        prompt
    }
    fn dismissed(&self) -> bool {
        self.kv.get_item(DISMISSED).as_deref() == Some("1")
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.dismissed() {
            self.phase = Phase::Hidden;
            return;
        }
        if self.checking || self.phase == Phase::Starring {
            return;
        }
        let Some(inbox) = Inbox::try_global(cx) else {
            self.phase = Phase::Hidden;
            return;
        };
        let request = inbox.read(cx).client().github_monocode_star_status();
        let initial = self.phase == Phase::Loading;
        let generation = self.generation;
        self.checking = true;
        self.check = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                this.checking = false;
                if this.generation != generation {
                    return;
                }
                if this.dismissed() {
                    this.phase = Phase::Hidden;
                } else {
                    match result {
                        Ok(GithubStarStatus::Starred) => this.phase = Phase::Hidden,
                        Ok(GithubStarStatus::NotStarred) => this.phase = Phase::Visible,
                        _ if initial => this.phase = Phase::Hidden,
                        _ => {}
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }
    fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        self.kv.set_item(DISMISSED, "1");
        self.phase = Phase::Hidden;
        cx.notify();
    }
    fn star(&mut self, cx: &mut Context<Self>) {
        if self.phase != Phase::Visible {
            return;
        }
        let Some(inbox) = Inbox::try_global(cx) else {
            return;
        };
        let request = inbox.read(cx).client().star_monocode_on_github();
        self.generation = self.generation.wrapping_add(1);
        self.phase = Phase::Starring;
        self.star = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                if result.is_ok() || this.dismissed() {
                    this.phase = Phase::Hidden;
                } else {
                    this.phase = Phase::Visible;
                    cx.open_url(URL);
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }
}

impl Render for Prompt {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id = window.window_handle().window_id();
        self.activations.entry(id).or_insert_with(|| {
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.refresh(cx);
                }
            })
        });
        if !matches!(self.phase, Phase::Visible | Phase::Starring) {
            return div().into_any_element();
        }
        let theme = Theme::of(cx).clone();
        let busy = self.phase == Phase::Starring;
        div()
            .flex()
            .items_center()
            .gap(u(2.))
            .mx(u(8.))
            .mt(u(4.))
            .rounded(u(6.))
            .bg(theme.accent(0.1))
            .child(
                div()
                    .id("github-star-action")
                    .debug_selector(|| "github-star-action".into())
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .justify_center()
                    .gap(u(8.))
                    .h(u(32.))
                    .pl(u(8.))
                    .cursor_pointer()
                    .text_color(theme.colors.accent)
                    .child(icon(IconName::Star).size(u(12.)))
                    .child(div().text_px(12.).medium().child(if busy {
                        "Starring..."
                    } else {
                        "Star on GitHub"
                    }))
                    .on_click(cx.listener(|this, _, _, cx| this.star(cx))),
            )
            .child(
                icon_button("github-star-dismiss", IconName::X)
                    .size(24.)
                    .icon_size(12.)
                    .tooltip("Don't show again")
                    .on_click(cx.listener(|this, _, _, cx| this.dismiss(cx))),
            )
            .into_any_element()
    }
}

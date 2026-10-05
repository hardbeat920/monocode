//! Port of `PlanSurface` in src/features/files/ui/FilePane.tsx: a plan
//! block opened in a tab, with a Markdown preview, an editable source, and
//! the Build button.

use std::rc::Rc;

use gpui::{
    AppContext as _, Context, Entity, EventEmitter, InteractiveElement, IntoElement, ParentElement,
    Render, StatefulInteractiveElement, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_base::input::{Textarea, TextareaState};
use gpui_component::input::InputEvent;
use monocode_core::block::{PlanBuildTarget, PlanStatus};
use monocode_core::{Block, Session};
use monocode_layout::FilePaneTab;
use monocode_layout::paths::is_remote_project_path;
use monocode_markdown::MarkdownView;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};
use monocode_view_transcript::threads::{
    ModelMenuSource, SecondOpinionButton, SecondOpinionEvent, SecondOpinionProps,
};

use crate::markdown_shell::{
    MarkdownViewMode, markdown_view_shell, remember_mode, remembered_mode,
};

/// What the plan tab asks its owner to do.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanSurfaceEvent {
    /// `onUpdatePlan`: the user edited the plan source.
    Update {
        session_id: String,
        block_id: String,
        text: String,
    },
    /// `onBuildPlan` with the selected provider, model, and settings.
    Build {
        session_id: String,
        block_id: String,
        target: Option<PlanBuildTarget>,
    },
}

/// The plan's session and block, when both still exist.
fn find_plan<'a>(
    file: &FilePaneTab,
    sessions: &'a [Session],
) -> (Option<&'a Session>, Option<&'a Block>) {
    let Some(plan) = &file.plan else {
        return (None, None);
    };
    let session = sessions
        .iter()
        .find(|session| session.id == plan.session_id);
    let block = session.and_then(|session| {
        session
            .blocks
            .iter()
            .find(|block| block.id == plan.block_id)
    });
    (session, block)
}

fn plan_status(block: &Block) -> Option<PlanStatus> {
    block.plan.as_ref().map(|plan| plan.status)
}

fn plan_locked(session: &Session, block: &Block) -> bool {
    is_remote_project_path(&session.cwd)
        || matches!(
            plan_status(block),
            Some(PlanStatus::Streaming | PlanStatus::Building | PlanStatus::Built)
        )
}

fn build_target_props(session: &Session, block: &Block) -> Option<SecondOpinionProps> {
    (!is_remote_project_path(&session.cwd)).then(|| {
        SecondOpinionProps::build_target(
            session.harness,
            Some(session.model.clone()),
            Some(session.model_settings.clone()),
            build_disabled(Some(session), block),
        )
    })
}

/// `buildDisabled`.
pub fn build_disabled(session: Option<&Session>, block: &Block) -> bool {
    session.is_some_and(|session| session.busy == Some(true))
        || block.text.trim().is_empty()
        || matches!(
            plan_status(block),
            Some(PlanStatus::Streaming | PlanStatus::Building | PlanStatus::Built)
        )
}

/// `buildLabel`.
pub fn build_label(block: &Block) -> &'static str {
    match plan_status(block) {
        Some(PlanStatus::Building) => "Building…",
        Some(PlanStatus::Built) => "Built",
        _ => "Build",
    }
}

/// The plan tab surface.
pub struct PlanSurface {
    file: FilePaneTab,
    sessions: Rc<Vec<Session>>,
    mode: MarkdownViewMode,
    preview: Entity<MarkdownView>,
    source: Entity<TextareaState>,
    model_source: Option<Rc<dyn ModelMenuSource>>,
    target_picker: Option<(Entity<SecondOpinionButton>, Subscription)>,
    _subscription: Subscription,
}

impl EventEmitter<PlanSurfaceEvent> for PlanSurface {}

impl PlanSurface {
    pub fn new(
        file: FilePaneTab,
        sessions: Rc<Vec<Session>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let text = find_plan(&file, &sessions)
            .1
            .map(|block| block.text.clone())
            .unwrap_or_default();
        let preview = cx.new(|cx| MarkdownView::with_text(text.clone(), cx));
        let source = cx.new(|cx| TextareaState::new(window, cx).default_value(text));
        let subscription = cx.subscribe(&source, |this, source, event: &InputEvent, cx| {
            if let InputEvent::Change = event
                && let Some(plan) = &this.file.plan
                && let (Some(session), Some(block)) = find_plan(&this.file, &this.sessions)
                && !plan_locked(session, block)
                && !session.is_busy()
            {
                cx.emit(PlanSurfaceEvent::Update {
                    session_id: plan.session_id.clone(),
                    block_id: plan.block_id.clone(),
                    text: source.read(cx).value().to_string(),
                });
            }
        });
        let mut this = Self {
            mode: remembered_mode(&file.path, cx),
            file,
            sessions,
            preview,
            source,
            model_source: None,
            target_picker: None,
            _subscription: subscription,
        };
        this.sync(window, cx);
        this
    }

    pub fn mode(&self) -> MarkdownViewMode {
        self.mode
    }

    pub fn set_model_source(&mut self, source: Rc<dyn ModelMenuSource>, cx: &mut Context<Self>) {
        self.model_source = Some(source.clone());
        if let Some((picker, _)) = &self.target_picker {
            picker.update(cx, |picker, cx| picker.set_source(source, cx));
        }
        cx.notify();
    }

    /// New session data: follow the block's text and status.
    pub fn set_sessions(
        &mut self,
        sessions: Rc<Vec<Session>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sessions = sessions;
        self.sync(window, cx);
        cx.notify();
    }

    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (session, block) = find_plan(&self.file, &self.sessions);
        let Some(block) = block else {
            return;
        };
        let locked = session.is_none_or(|session| plan_locked(session, block));
        let text = block.text.clone();
        let streaming = block.streaming == Some(true);
        self.preview.update(cx, |preview, cx| {
            preview.set_text(&text, cx);
            preview.set_streaming(streaming, cx);
        });
        self.source.update(cx, |source, cx| {
            if source.value() != text.as_str() {
                source.set_value(text, window, cx);
            }
            source.set_readonly(locked, cx);
        });
    }

    pub fn set_mode(&mut self, mode: MarkdownViewMode, cx: &mut Context<Self>) {
        remember_mode(&self.file.path, mode, cx);
        self.mode = mode;
        cx.notify();
    }
}

impl Render for PlanSurface {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let sessions = self.sessions.clone();
        let (session, block) = find_plan(&self.file, &sessions);
        let (Some(block), Some(plan)) = (block, self.file.plan.clone()) else {
            return div()
                .flex()
                .size_full()
                .items_center()
                .justify_center()
                .p(u(24.))
                .text_px(theme.text.body)
                .text_color(theme.content(0.70))
                .child("This plan is no longer in the session.")
                .into_any_element();
        };
        let disabled = build_disabled(session, block);
        let label = build_label(block);
        let locked = session.is_none_or(|session| plan_locked(session, block));
        let target_props = session.and_then(|session| build_target_props(session, block));
        let target_picker =
            if let (Some(props), Some(source)) = (target_props, self.model_source.clone()) {
                if let Some((picker, _)) = &self.target_picker {
                    picker.update(cx, |picker, cx| picker.set_props(props, cx));
                    Some(picker.clone())
                } else {
                    let picker = cx.new(|cx| SecondOpinionButton::new(props, source, cx));
                    let target_plan = plan.clone();
                    let subscription = cx.subscribe(&picker, move |_, _, event, cx| {
                        let SecondOpinionEvent::Pick(target) = event;
                        cx.emit(PlanSurfaceEvent::Build {
                            session_id: target_plan.session_id.clone(),
                            block_id: target_plan.block_id.clone(),
                            target: Some(target.clone()),
                        });
                    });
                    self.target_picker = Some((picker.clone(), subscription));
                    Some(picker)
                }
            } else {
                None
            };
        let background = theme.colors.background_base;
        let hover = theme.content(0.90);
        let build = div()
            .id("build-plan")
            .flex()
            .h(u(24.))
            .items_center()
            .gap(u(6.))
            .rounded(u(theme.radius.md))
            .bg(theme.colors.content)
            .px(u(10.))
            .font_family(theme.fonts.sans.clone())
            .text_px(theme.text.caption)
            .medium()
            .text_color(background)
            .when(disabled, |button| button.opacity(0.4))
            .when(!disabled, |button| {
                button
                    .hover(move |style| style.bg(hover))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(PlanSurfaceEvent::Build {
                            session_id: plan.session_id.clone(),
                            block_id: plan.block_id.clone(),
                            target: None,
                        })
                    }))
            })
            .child(icon(IconName::Play).size(u(12.)).text_color(background))
            .child(label);
        let source = div()
            .size_full()
            .px(u(20.))
            .pb(u(20.))
            .pt(u(56.))
            .font_family(theme.fonts.mono.clone())
            .text_px(theme.text.body)
            .line_height(u(24.))
            .text_color(theme.colors.content)
            .when(locked, |source| source.opacity(0.7))
            .child(Textarea::new(&self.source));
        let preview = div()
            .id("plan-preview")
            .size_full()
            .overflow_y_scroll()
            .child(div().px(u(24.)).py(u(32.)).child(self.preview.clone()));
        let weak = cx.entity().downgrade();
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .min_w_0()
            .child(
                markdown_view_shell(
                    self.mode,
                    move |mode, _, cx| {
                        weak.update(cx, |this, cx| this.set_mode(mode, cx)).ok();
                    },
                    preview,
                    source,
                )
                .actions(
                    div()
                        .flex()
                        .items_center()
                        .child(build)
                        .children(target_picker),
                ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::BlockRole;
    use monocode_core::block::PlanBlockMeta;

    fn plan_block(text: &str, status: PlanStatus) -> Block {
        let mut block = Block::new("plan", BlockRole::Assistant, text);
        block.plan = Some(PlanBlockMeta {
            status,
            ..Default::default()
        });
        block
    }

    #[test]
    fn labels_and_disables_build_like_the_typescript() {
        let ready = plan_block("1. Do it", PlanStatus::Ready);
        assert_eq!(build_label(&ready), "Build");
        assert!(!build_disabled(None, &ready));
        assert!(build_disabled(None, &plan_block("  ", PlanStatus::Ready)));
        let building = plan_block("x", PlanStatus::Building);
        assert_eq!(build_label(&building), "Building…");
        assert!(build_disabled(None, &building));
        assert_eq!(build_label(&plan_block("x", PlanStatus::Built)), "Built");
        assert!(build_disabled(
            None,
            &plan_block("x", PlanStatus::Streaming)
        ));
    }

    #[test]
    fn local_build_targets_keep_the_current_model_and_effort_while_remote_plans_stay_locked() {
        use monocode_core::HarnessId;
        let block = plan_block("Build the app", PlanStatus::Ready);
        let mut session = Session::blank("tab", HarnessId::Codex, "codex:current", "/repo");
        session
            .model_settings
            .insert("effort".into(), "high".into());
        let props = build_target_props(&session, &block).unwrap();
        assert_eq!(props.from, HarnessId::Codex);
        assert_eq!(props.from_model.as_deref(), Some("codex:current"));
        assert_eq!(props.from_settings, Some(session.model_settings.clone()));
        assert!(!props.disabled);
        assert!(props.include_current);
        assert!(!plan_locked(&session, &block));
        session.busy = Some(true);
        assert!(build_target_props(&session, &block).unwrap().disabled);
        session.cwd = "remote://env/repo".into();
        assert!(build_target_props(&session, &block).is_none());
        assert!(plan_locked(&session, &block));
    }
}

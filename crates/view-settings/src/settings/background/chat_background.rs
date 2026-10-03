//! Port of the apply half of src/features/settings/model/newThreadBackgroundEffects.ts
//! (`applyPreparedNewThreadBackground`, `clearPreparedNewThreadBackground`)
//! and of `applyChatBackground`, `renderChatBackground`, and
//! `chatBackgroundSrc` in appearance.ts.
//!
//! The webview set `--chat-background-image` and three classes on the root
//! element. `ChatBackground` is a GPUI global holding the same state, and
//! chat panes observe it: the image to draw, whether it is ready to fade in
//! (`chat-background-effect-ready`), and whether Haze is on
//! (`chat-background-gradient-blur`).

use std::sync::Arc;

use gpui::{App, BorrowAppContext as _, Global, ImageSource, RenderImage, SharedString};
use monocode_core::appearance::NewThreadBackgroundEffect;

use super::service::BackgroundEffects;

/// What a background draws: the file itself, which keeps GIF animation, or
/// an effect rendered from it.
#[derive(Clone, Debug, PartialEq)]
pub enum BackgroundImage {
    Original(SharedString),
    Prepared(Arc<RenderImage>),
}

impl BackgroundImage {
    /// The source for `gpui::img`.
    pub fn source(&self) -> ImageSource {
        match self {
            BackgroundImage::Original(path) => {
                ImageSource::from(std::path::PathBuf::from(path.as_ref()))
            }
            BackgroundImage::Prepared(image) => ImageSource::Render(image.clone()),
        }
    }

    pub fn is_original(&self) -> bool {
        matches!(self, BackgroundImage::Original(_))
    }
}

/// The global chat background, as the chat panes draw it.
#[derive(Default)]
pub struct ChatBackground {
    /// `has-chat-background`.
    pub path: Option<SharedString>,
    /// `--chat-background-image`.
    pub image: Option<BackgroundImage>,
    /// `chat-background-effect-ready`.
    pub effect_ready: bool,
    /// `chat-background-gradient-blur`.
    pub gradient_blur: bool,
    /// `appliedRevision`: a newer apply drops an older one's late result.
    applied_revision: u64,
    /// `chatBackgroundRevision`: bumps each time a path is applied, so the
    /// same file reloads after it is replaced.
    image_revision: i64,
}

impl Global for ChatBackground {}

impl ChatBackground {
    pub fn global(cx: &App) -> Option<&ChatBackground> {
        cx.try_global::<ChatBackground>()
    }

    fn update<R>(cx: &mut App, f: impl FnOnce(&mut ChatBackground, &mut App) -> R) -> R {
        if !cx.has_global::<ChatBackground>() {
            cx.set_global(ChatBackground::default());
        }
        cx.update_global(f)
    }

    /// `chatBackgroundRevision`.
    pub fn image_revision(cx: &App) -> i64 {
        Self::global(cx)
            .map(|this| this.image_revision)
            .unwrap_or(0)
    }

    /// `chatBackgroundSrc`: the path and the revision to reload it by.
    pub fn src(path: Option<&str>, cx: &App) -> Option<(SharedString, i64)> {
        path.map(|path| {
            (
                SharedString::from(path.to_string()),
                Self::image_revision(cx),
            )
        })
    }

    /// `clearPreparedNewThreadBackground`.
    pub fn clear(cx: &mut App) {
        Self::update(cx, |this, _| {
            this.applied_revision += 1;
            this.image = None;
            this.effect_ready = false;
            this.gradient_blur = false;
        });
    }

    /// `applyChatBackground`: show `path`, or clear the background.
    pub fn apply_chat_background(
        path: Option<&str>,
        effect: NewThreadBackgroundEffect,
        light: bool,
        cx: &mut App,
    ) {
        Self::update(cx, |this, _| {
            this.path = path.map(|path| SharedString::from(path.to_string()))
        });
        let Some(path) = path else {
            Self::clear(cx);
            return;
        };
        Self::update(cx, |this, _| this.image_revision += 1);
        Self::render_chat_background(path, effect, light, cx);
    }

    /// `applyNewThreadBackgroundEffect` and the scheme change in
    /// `applyThemePreference`: redraw the current path with `effect`.
    pub fn rerender(effect: NewThreadBackgroundEffect, light: bool, cx: &mut App) {
        let path = Self::global(cx).and_then(|this| this.path.clone());
        if let Some(path) = path {
            Self::render_chat_background(&path, effect, light, cx);
        }
    }

    /// `renderChatBackground`.
    fn render_chat_background(
        path: &str,
        effect: NewThreadBackgroundEffect,
        light: bool,
        cx: &mut App,
    ) {
        let source_key = format!("{path}?v={}", Self::image_revision(cx));
        Self::apply_prepared(&source_key, path, effect, light, cx);
    }

    /// `applyPreparedNewThreadBackground`.
    pub fn apply_prepared(
        source_key: &str,
        path: &str,
        effect: NewThreadBackgroundEffect,
        light: bool,
        cx: &mut App,
    ) {
        let revision = Self::update(cx, |this, _| {
            this.applied_revision += 1;
            this.effect_ready = false;
            this.gradient_blur = effect == NewThreadBackgroundEffect::GradientBlur;
            this.applied_revision
        });
        let original = BackgroundImage::Original(SharedString::from(path.to_string()));
        if matches!(
            effect,
            NewThreadBackgroundEffect::None | NewThreadBackgroundEffect::GradientBlur
        ) {
            Self::update(cx, |this, _| this.image = Some(original));
            Self::ready_next_frame(revision, cx);
            return;
        }
        let prepared = BackgroundEffects::prepare(source_key, path, effect, light, cx);
        cx.spawn(async move |cx| {
            let result = prepared.await;
            cx.update(|cx| {
                let current =
                    Self::global(cx).is_some_and(|this| this.applied_revision == revision);
                if !current {
                    return;
                }
                match result {
                    Ok(image) => {
                        Self::update(cx, |this, _| {
                            this.image = Some(BackgroundImage::Prepared(image))
                        });
                        Self::ready_next_frame(revision, cx);
                    }
                    Err(_) => Self::update(cx, |this, _| {
                        this.image = Some(original);
                        this.effect_ready = true;
                    }),
                }
            });
        })
        .detach();
    }

    /// `requestAnimationFrame(...)`: mark the image ready once the frame
    /// that shows it has been drawn, unless a newer apply came first.
    fn ready_next_frame(revision: u64, cx: &mut App) {
        cx.defer(move |cx| {
            Self::update(cx, |this, _| {
                if this.applied_revision == revision {
                    this.effect_ready = true;
                }
            });
        });
    }
}

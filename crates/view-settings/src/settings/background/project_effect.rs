//! Port of src/features/projects/ui/useProjectBackgroundEffect.ts: a
//! pane-local background image with an effect applied, leaving the global
//! chat background untouched.
//!
//! The hook becomes a small entity. Its owner calls [`ProjectBackgroundEffect::resolve`]
//! while rendering and observes the entity, which notifies once a prepared
//! image arrives.

use gpui::{Context, SharedString, Task};
use monocode_core::appearance::NewThreadBackgroundEffect;

use super::chat_background::BackgroundImage;
use super::service::BackgroundEffects;

#[derive(Default)]
pub struct ProjectBackgroundEffect {
    /// `prepared`: the last finished image and the key it was made for.
    prepared: Option<(String, BackgroundImage)>,
    /// The job for the key being prepared. Replacing it cancels the old one,
    /// the effect cleanup's `cancelled = true`.
    pending: Option<(String, Task<()>)>,
}

impl ProjectBackgroundEffect {
    pub fn new() -> Self {
        Self::default()
    }

    /// The image for `path` with `effect`: the original file for None and
    /// Haze, the prepared image once it is ready, and `None` while it is
    /// being prepared.
    pub fn resolve(
        &mut self,
        path: Option<&str>,
        effect: NewThreadBackgroundEffect,
        revision: i64,
        light: bool,
        cx: &mut Context<Self>,
    ) -> Option<BackgroundImage> {
        let path = path?;
        let src = SharedString::from(path.to_string());
        if matches!(
            effect,
            NewThreadBackgroundEffect::None | NewThreadBackgroundEffect::GradientBlur
        ) {
            self.pending = None;
            return Some(BackgroundImage::Original(src));
        }
        let source_key = format!("{path}?v={revision}");
        let theme_key = match effect {
            NewThreadBackgroundEffect::Dither => false,
            _ => light,
        };
        let key = format!("{source_key}:{}:{theme_key}", effect.as_str());
        if let Some((prepared_key, image)) = &self.prepared
            && *prepared_key == key
        {
            return Some(image.clone());
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|(pending, _)| *pending == key)
        {
            return None;
        }
        let prepared = BackgroundEffects::prepare(&source_key, path, effect, theme_key, cx);
        let job_key = key.clone();
        let task = cx.spawn(async move |this, cx| {
            let image = match prepared.await {
                Ok(image) => BackgroundImage::Prepared(image),
                Err(_) => BackgroundImage::Original(src),
            };
            this.update(cx, |this, cx| {
                this.pending = None;
                this.prepared = Some((job_key, image));
                cx.notify();
            })
            .ok();
        });
        self.pending = Some((key, task));
        None
    }
}

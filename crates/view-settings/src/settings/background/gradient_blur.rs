//! Port of src/features/settings/ui/GradientBlurBackground.tsx and the
//! `.gradient-blur-background` rules in src/styles/index.css: the Haze
//! treatment shared by the chat pane and both background previews.
//!
//! CSS drew Haze with two blurred, masked copies of the artwork and two
//! gradient overlays, all in the element's box. GPUI cannot filter or mask
//! an image, so the element measures its box and the layers are painted
//! into one image of that size on a background thread
//! ([`super::effects::haze_pixels`]), tinted with the current theme
//! background.

use std::sync::Arc;

use gpui::{
    App, Context, ElementId, Hsla, ImageSource, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderImage, RenderOnce, SharedString, Styled as _, Task, Window, canvas,
    div, img, prelude::FluentBuilder as _,
};
use monocode_ui::Theme;

use super::effects::HazeVariant;
use super::service::BackgroundEffects;

/// `--color-background-base` as 8-bit RGB.
pub fn background_rgb(color: Hsla) -> [u8; 3] {
    let rgb = color.to_rgb();
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    [byte(rgb.r), byte(rgb.g), byte(rgb.b)]
}

/// One Haze element's measured box, its request, and its image.
#[derive(Default)]
pub struct HazeState {
    /// The box in CSS px, from the last frame.
    size: Option<(f64, f64)>,
    key: Option<String>,
    image: Option<Arc<RenderImage>>,
    task: Option<Task<()>>,
}

impl HazeState {
    pub fn image(&self) -> Option<&Arc<RenderImage>> {
        self.image.as_ref()
    }

    fn resolve(
        &mut self,
        path: &str,
        revision: i64,
        variant: HazeVariant,
        background: [u8; 3],
        cx: &mut Context<Self>,
    ) -> Option<Arc<RenderImage>> {
        let (width, height) = self.size?;
        let source_key = format!("{path}?v={revision}");
        let key = format!("{source_key}:{variant:?}:{background:?}:{width}x{height}");
        if self.key.as_deref() == Some(key.as_str()) {
            return self.image.clone();
        }
        self.key = Some(key);
        let prepared = BackgroundEffects::prepare_haze(
            &source_key,
            path,
            variant,
            background,
            (width, height),
            cx,
        );
        self.task = Some(cx.spawn(async move |this, cx| {
            let image = prepared.await.ok();
            this.update(cx, |this, cx| {
                this.image = image;
                cx.notify();
            })
            .ok();
        }));
        // Keep the previous image while a resized one renders.
        self.image.clone()
    }
}

#[derive(IntoElement)]
pub struct GradientBlurBackground {
    id: ElementId,
    path: SharedString,
    revision: i64,
    variant: HazeVariant,
    opacity: f32,
}

/// Haze over `path`, filling its parent.
pub fn gradient_blur_background(
    id: impl Into<ElementId>,
    path: impl Into<SharedString>,
    revision: i64,
    variant: HazeVariant,
) -> GradientBlurBackground {
    GradientBlurBackground {
        id: id.into(),
        path: path.into(),
        revision,
        variant,
        opacity: 1.0,
    }
}

impl GradientBlurBackground {
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }
}

impl RenderOnce for GradientBlurBackground {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let background = background_rgb(Theme::of(cx).colors.background_base);
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| HazeState::default());
        let (path, revision, variant) = (self.path.clone(), self.revision, self.variant);
        let image = state.update(cx, |state, cx| {
            state.resolve(&path, revision, variant, background, cx)
        });
        let probe_state = state.clone();
        let probe = canvas(
            move |bounds, window, cx| {
                let rem = f32::from(window.rem_size());
                let css = |value: gpui::Pixels| (f32::from(value) / rem * 16.0).round() as f64;
                let size = (css(bounds.size.width), css(bounds.size.height));
                if size.0 >= 1.0 && size.1 >= 1.0 && probe_state.read(cx).size != Some(size) {
                    probe_state.update(cx, |state, cx| {
                        state.size = Some(size);
                        cx.notify();
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .overflow_hidden()
            .opacity(self.opacity)
            .debug_selector(|| "gradient-blur-background".into())
            .child(probe)
            .when_some(image, |el, image| {
                el.child(img(ImageSource::Render(image)).size_full())
            })
    }
}

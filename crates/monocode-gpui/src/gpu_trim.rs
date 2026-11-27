//! Gives GPU memory back once the app goes quiet. Sprite atlases keep every
//! glyph, icon and image any frame drew, and renderers keep the buffers of
//! their busiest frame, so without this the footprint only ever grows.
//! Trimming clears them and redraws once; only what is on screen comes back.
use gpui::App;
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

const CHECK: Duration = Duration::from_secs(5);
/// How long every window has to be in the background first.
const BACKGROUND_AFTER: Duration = Duration::from_secs(10);
/// How long an active window has to go without typing first.
const QUIET_AFTER: Duration = Duration::from_secs(120);
/// Agents keep drawing while nobody types; trim again after this long.
const AGAIN_AFTER: Duration = Duration::from_secs(300);

pub(crate) fn start(cx: &mut App) {
    let typed = Rc::new(Cell::new(Instant::now()));
    cx.observe_keystrokes({
        let typed = typed.clone();
        move |_, _, _| typed.set(Instant::now())
    })
    .detach();
    cx.spawn(async move |cx| {
        let mut activated = Instant::now();
        let mut inactive_since = None;
        let mut trimmed: Option<Instant> = None;
        loop {
            cx.background_executor().timer(CHECK).await;
            let active = cx.update(crate::pulse::app_active);
            let now = Instant::now();
            if active {
                if inactive_since.take().is_some() {
                    activated = now;
                }
            } else {
                inactive_since.get_or_insert(now);
            }
            let activity = typed.get().max(activated);
            let quiet = match inactive_since {
                Some(since) => now.duration_since(since.max(activity)) >= BACKGROUND_AFTER,
                None => now.duration_since(activity) >= QUIET_AFTER,
            };
            let due =
                trimmed.is_none_or(|at| at < activity || now.duration_since(at) >= AGAIN_AFTER);
            if quiet && due {
                trimmed = Some(now);
                cx.update(trim);
            }
        }
    })
    .detach();
}

fn trim(cx: &mut App) {
    #[cfg(target_os = "macos")]
    gpui_apple::metal_renderer::trim();
    cx.clear_sprite_atlases();
}

#[cfg(test)]
mod tests {
    use gpui::{ImageSource, IntoElement, RenderImage, TestAppContext, img, point, px, size};
    use std::sync::Arc;

    #[gpui::test]
    fn trimming_empties_the_sprite_atlas(cx: &mut TestAppContext) {
        let frame = image::Frame::new(image::RgbaImage::from_pixel(1, 1, image::Rgba([0; 4])));
        let image = Arc::new(RenderImage::new(vec![frame]));
        let window = cx.add_empty_window();
        window.draw(point(px(0.), px(0.)), size(px(10.), px(10.)), |_, _| {
            img(ImageSource::Render(image.clone())).into_any_element()
        });
        assert!(window.update(|window, _| window.has_image_atlas_entry(&image)));

        let app: &TestAppContext = window;
        app.update(super::trim);
        assert!(!window.update(|window, _| window.has_image_atlas_entry(&image)));
    }
}

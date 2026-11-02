//! Scratch: renders the composer and sent messages with Metal and writes PNGs.
use super::*;
use gpui::{HeadlessAppContext, size};

fn flatten(image: image::RgbaImage, path: &str) {
    let mut flat = image::RgbaImage::from_pixel(
        image.width(),
        image.height(),
        image::Rgba([20, 20, 20, 255]),
    );
    image::imageops::overlay(&mut flat, &image, 0, 0);
    flat.save(path).unwrap();
}

struct Frame(gpui::Entity<crate::input::Input>);
impl gpui::Render for Frame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_4()
            .text_color(crate::theme::content(1.))
            .child(self.0.clone())
    }
}

fn headless() -> HeadlessAppContext {
    HeadlessAppContext::with_platform(
        Arc::new(gpui_apple::AppleTextSystem::new()),
        Arc::new(crate::assets::Assets),
        || {
            Ok(Some(Box::new(
                gpui_apple::metal_renderer::MetalHeadlessRenderer::new(),
            )))
        },
    )
}

#[test]
#[ignore = "Requires a Metal device; writes screenshots"]
fn markdown_shots() {
    let dir = std::env::var("MD_SHOTS").unwrap_or_else(|_| "/tmp/md-shots".into());
    std::fs::create_dir_all(&dir).unwrap();
    let mut cx = headless();
    cx.update(|cx| {
        cx.set_reduce_motion(true);
        cx.set_global(crate::settings::Preferences::default());
    });

    let composer = |cx: &mut HeadlessAppContext, name: &str, text: &str, height: f32| {
        let text = text.to_string();
        let window = cx
            .open_window(size(px(560.), px(height)), move |_, cx| {
                let input = cx.new(|cx| {
                    let mut input = crate::input::Input::new(cx);
                    input.set_font_size(16.);
                    input.set_line_height(24.);
                    input.set_min_height(48.);
                    input.set_max_height(400.);
                    input.enable_inline_commands(cx);
                    input.set_text(text.clone(), cx);
                    input
                });
                cx.new(|_| Frame(input))
            })
            .unwrap();
        for _ in 0..4 {
            cx.run_until_parked();
            cx.update_window(window.into(), |_, w, _| w.refresh())
                .unwrap();
        }
        let image = cx.capture_screenshot(window.into()).unwrap();
        flatten(image, &format!("{dir}/{name}.png"));
    };
    composer(
        &mut cx,
        "composer-styled",
        "## Release plan\n**Ship** the *markdown* composer, see `input/markdown.rs`\n- [x] styling while you type\n- [ ] list continuation\n> text stays exactly as typed\n```yaml\n- name: build\n```",
        260.,
    );

    let window = cx
        .open_window(size(px(760.), px(640.)), |_, cx| {
            cx.new(|_| {
                let mut view = TranscriptView::new();
                view.chat_layout = true;
                view
            })
        })
        .unwrap();
    let row = |role: &str, text: &str| Row {
        role: role.into(),
        text: text.into(),
        first: true,
        sent_at: None,
    };
    window
        .update(&mut cx, |view, _, cx| {
            view.patch(
                0,
                vec![
                    row("user", "fix the build please, it fails on CI"),
                    row("assistant", "Looking at the workflow now."),
                    row(
                        "user",
                        "## Plan\n1. Reproduce the **CI failure** locally\n2. Pin `rustfmt` to the CI version\n- [ ] update the docs\n> keep the diff small\n\n```sh\ncargo fmt --check > fmt.txt\n```\nThen **push** and *watch* the run.",
                    ),
                ],
                cx,
            );
        })
        .unwrap();
    for _ in 0..6 {
        cx.run_until_parked();
        cx.advance_clock(std::time::Duration::from_millis(400));
        cx.update_window(window.into(), |_, w, _| w.refresh())
            .unwrap();
    }
    let image = cx.capture_screenshot(window.into()).unwrap();
    flatten(image, &format!("{dir}/messages-markdown.png"));
    window
        .update(&mut cx, |view, _, cx| view.toggle_prompt(2, cx))
        .unwrap();
    for _ in 0..6 {
        cx.run_until_parked();
        cx.advance_clock(std::time::Duration::from_millis(400));
        cx.update_window(window.into(), |_, w, _| w.refresh())
            .unwrap();
    }
    let image = cx.capture_screenshot(window.into()).unwrap();
    flatten(image, &format!("{dir}/messages-markdown-expanded.png"));
}

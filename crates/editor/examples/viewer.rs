//! Opens an image in `ImageView` or a PDF in `PdfView`, chosen from the bytes.
//!
//! ```text
//! cargo run -p monocode-editor --example viewer -- <path> [--zoom <scale>] [--light] [--screenshot <png>]
//! ```

use std::{path::PathBuf, time::Duration};

use gpui::{
    AnyView, App, AppContext as _, Bounds, Context, IntoElement, ParentElement, Render, Styled,
    Window, WindowBounds, WindowOptions, div, px, size,
};
use monocode_editor::{EditorTheme, ImageView, PdfView, viewer::Zoom};

struct Shell {
    view: AnyView,
    theme: EditorTheme,
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(self.theme.panel_background)
            .text_color(self.theme.foreground)
            .child(self.view.clone())
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut path = PathBuf::new();
    let mut zoom = None;
    let mut light = false;
    let mut screenshot: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--zoom" => zoom = args.next().and_then(|value| value.parse::<f32>().ok()),
            "--light" => light = true,
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            _ => path = PathBuf::from(arg),
        }
    }
    let bytes = std::fs::read(&path).expect("read file");
    let theme = if light {
        EditorTheme::light()
    } else {
        EditorTheme::dark()
    };

    gpui_platform::application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        monocode_editor::init(cx);
        let bounds = Bounds::centered(None, size(px(900.), px(700.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_, cx| {
                    let view: AnyView = if monocode_editor::viewer::is_pdf_bytes(&bytes) {
                        let view = cx.new(|cx| PdfView::new(bytes, theme.clone(), cx));
                        if let Some(zoom) = zoom {
                            view.update(cx, |view, cx| view.set_zoom(Zoom::Scale(zoom), cx));
                        }
                        view.into()
                    } else {
                        let path = path.to_string_lossy().to_string();
                        let view = cx.new(|_| ImageView::new(path, bytes, theme.clone()));
                        if let Some(zoom) = zoom {
                            view.update(cx, |view, cx| view.set_zoom(Zoom::Scale(zoom), cx));
                        }
                        view.into()
                    };
                    cx.new(|_| Shell { view, theme })
                },
            )
            .expect("open window");

        cx.spawn(async move |cx| {
            let Some(screenshot) = screenshot else {
                return;
            };
            // Let the document open and the visible pages draw.
            cx.background_executor()
                .timer(Duration::from_millis(2500))
                .await;
            let any_window: gpui::AnyWindowHandle = window.into();
            let result = any_window.update(cx, |_, window, cx| {
                window.draw(cx).clear();
                window
                    .render_to_image()
                    .map(|image| image.save(&screenshot))
            });
            match result {
                Ok(Ok(Ok(()))) => eprintln!("wrote {}", screenshot.display()),
                other => eprintln!("screenshot failed: {other:?}"),
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
}

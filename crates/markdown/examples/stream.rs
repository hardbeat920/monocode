//! Streams a sample reply into a `MarkdownView` at typing speed.
//!
//! ```sh
//! cargo run -p monocode-markdown --example stream
//! cargo run -p monocode-markdown --example stream -- --screenshot out.png
//! cargo run -p monocode-markdown --example stream -- --screenshot out.png \
//!     --light --width 760 --height 1400 --offset 900 --stream-to 0.6
//! ```
//!
//! `--screenshot` renders offscreen with the native text system and the Metal
//! headless renderer (macOS), writes a PNG at 2x scale, and exits.
//! `--stream-to F` streams the first fraction F of the sample, lets the
//! reveal catch up, appends a few more words, and captures mid-fade.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    App, AppContext, Bounds, Context, Entity, Hsla, IntoElement, ParentElement, Pixels, Render,
    Styled, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};
use monocode_markdown::{MarkdownStyle, MarkdownView};

const SAMPLE: &str = include_str!("sample.md");

/// A small SVG badge, inlined as a data URL so the sample shows an image
/// without touching the network or the disk.
const BADGE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="360" height="96" viewBox="0 0 360 96"><defs><linearGradient id="g" x1="0" x2="1"><stop offset="0" stop-color="#f9a8c9"/><stop offset="1" stop-color="#7dd3fc"/></linearGradient></defs><rect width="360" height="96" rx="14" fill="url(#g)"/><text x="24" y="58" font-family="Helvetica" font-size="30" font-weight="700" fill="#171717">monocode-markdown</text></svg>"##;

fn sample() -> String {
    SAMPLE.replace(
        "BADGE_IMAGE",
        &format!("data:image/svg+xml;base64,{}", base64(BADGE_SVG.as_bytes())),
    )
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

struct Demo {
    markdown: Entity<MarkdownView>,
    background: Hsla,
    offset: Pixels,
}

impl Render for Demo {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("scroll")
            .size_full()
            .bg(self.background)
            .overflow_y_scroll()
            .child(
                div()
                    .relative()
                    .top(-self.offset)
                    .px(px(28.))
                    .py(px(24.))
                    .max_w(px(780.))
                    .child(self.markdown.clone()),
            )
    }
}

struct Args {
    screenshot: Option<String>,
    light: bool,
    width: f32,
    height: f32,
    offset: f32,
    stream_to: Option<f32>,
    /// How long after the last append the mid-stream capture happens.
    fade_ms: f32,
    /// Drag a selection from the first paragraph into the list, and hover the
    /// first link, before capturing.
    interact: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        screenshot: None,
        light: false,
        width: 760.,
        height: 1000.,
        offset: 0.,
        stream_to: None,
        fade_ms: 120.,
        interact: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--screenshot" => args.screenshot = it.next(),
            "--light" => args.light = true,
            "--width" => args.width = number(&mut it, "--width"),
            "--height" => args.height = number(&mut it, "--height"),
            "--offset" => args.offset = number(&mut it, "--offset"),
            "--stream-to" => args.stream_to = Some(number(&mut it, "--stream-to")),
            "--fade-ms" => args.fade_ms = number(&mut it, "--fade-ms"),
            "--interact" => args.interact = true,
            other => panic!("unknown argument {other}"),
        }
    }
    args
}

fn number(it: &mut impl Iterator<Item = String>, name: &str) -> f32 {
    it.next()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| panic!("{name} needs a number"))
}

fn theme(light: bool) -> (MarkdownStyle, Hsla) {
    if light {
        (MarkdownStyle::light(), rgb(0xf7f7f7).into())
    } else {
        (MarkdownStyle::dark(), rgb(0x171717).into())
    }
}

/// Deterministic chunk sizes between 1 and 12 bytes, like token bursts.
struct Chunks(u64);

impl Chunks {
    fn next(&mut self) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        1 + (self.0 >> 33) as usize % 12
    }
}

fn next_boundary(text: &str, mut ix: usize) -> usize {
    ix = ix.min(text.len());
    while !text.is_char_boundary(ix) {
        ix += 1;
    }
    ix
}

fn main() {
    let args = parse_args();
    if args.screenshot.is_some() {
        screenshot(&args);
        return;
    }
    gpui_platform::application().run(move |cx: &mut App| {
        monocode_markdown::init(cx);
        let (style, background) = theme(args.light);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(args.width), px(args.height)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |_, cx| {
                    let markdown = cx.new(|cx| {
                        let mut view = MarkdownView::new(cx);
                        view.set_style(style, cx);
                        view.on_link_click(|link, _, cx| {
                            eprintln!("link clicked: {}", link.url);
                            cx.open_url(&link.url);
                        });
                        view
                    });
                    cx.new(|_| Demo {
                        markdown,
                        background,
                        offset: px(0.),
                    })
                },
            )
            .expect("open window");
        let markdown = window
            .read_with(cx, |demo, _| demo.markdown.clone())
            .expect("read demo");
        let text = sample();
        cx.spawn(async move |cx| {
            markdown.update(cx, |view, cx| view.set_streaming(true, cx));
            let mut chunks = Chunks(7);
            let mut at = 0;
            while at < text.len() {
                let end = next_boundary(&text, at + chunks.next());
                let delta = text[at..end].to_string();
                markdown.update(cx, |view, cx| view.push_str(&delta, cx));
                at = end;
                cx.background_executor()
                    .timer(Duration::from_millis(30))
                    .await;
            }
            markdown.update(cx, |view, cx| view.set_streaming(false, cx));
        })
        .detach();
        cx.activate(true);
    });
}

fn screenshot(args: &Args) {
    let path = args.screenshot.clone().expect("--screenshot path");
    let platform = gpui_platform::current_platform(true);
    let mut cx =
        gpui::HeadlessAppContext::with_platform(platform.text_system(), Arc::new(()), || {
            gpui_platform::current_headless_renderer()
        });
    cx.update(monocode_markdown::init);
    // Load the grammars up front so the capture has colors.
    monocode_markdown::highlight::syntaxes_blocking();

    let (style, background) = theme(args.light);
    let text = sample();
    let streaming = args.stream_to.is_some();
    let window = cx
        .open_window(size(px(args.width), px(args.height)), |_, cx| {
            let markdown = cx.new(|cx| {
                let mut view = if streaming {
                    MarkdownView::new(cx)
                } else {
                    MarkdownView::with_text(text.clone(), cx)
                };
                view.set_style(style, cx);
                view
            });
            cx.new(|_| Demo {
                markdown,
                background,
                offset: px(args.offset),
            })
        })
        .expect("open window");
    let view = cx
        .read_window(&window, |demo, cx| demo.read(cx).markdown.clone())
        .expect("demo");

    let draw = |cx: &mut gpui::HeadlessAppContext| {
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
        })
        .expect("draw");
        cx.run_until_parked();
    };

    if let Some(fraction) = args.stream_to {
        let cut = next_boundary(&text, (text.len() as f32 * fraction) as usize);
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.set_streaming(true, cx);
                view.set_text(&text[..cut], cx);
            })
        });
        // Let the reveal catch up in real time.
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(2500) {
            draw(&mut cx);
            std::thread::sleep(Duration::from_millis(16));
        }
        // A few more words arrive; capture while they fade in.
        let more = next_boundary(&text, cut + 120);
        cx.update(|cx| view.update(cx, |view, cx| view.set_text(&text[..more], cx)));
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs_f32(args.fade_ms / 1000.) {
            draw(&mut cx);
            std::thread::sleep(Duration::from_millis(16));
        }
    } else {
        for _ in 0..4 {
            draw(&mut cx);
        }
    }
    if args.interact {
        interact(&mut cx, window, &view, &draw);
    }
    draw(&mut cx);
    let image = cx
        .capture_screenshot(window.into())
        .expect("capture screenshot");
    image.save(&path).expect("save png");
    eprintln!("wrote {path} ({}x{})", image.width(), image.height());
}

/// Drag-select from the first paragraph into the third text element, then
/// rest the pointer on the first link.
fn interact(
    cx: &mut gpui::HeadlessAppContext,
    window: gpui::WindowHandle<Demo>,
    view: &Entity<MarkdownView>,
    draw: &dyn Fn(&mut gpui::HeadlessAppContext),
) {
    use gpui::{
        Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    };
    let texts = cx.read_entity(view, |view, _| view.rendered_text());
    let at = |ix: usize, offset: usize| {
        let text = &texts[ix];
        let p = text.layout.position_for_index(offset).expect("position");
        gpui::point(p.x + px(1.), p.y + text.layout.line_height() / 2.)
    };
    let start = at(1, "I read the ".len());
    let end = at(3, "Parse incr".len());
    let send = |cx: &mut gpui::HeadlessAppContext, event: PlatformInput| {
        cx.update_window(window.into(), |_, window, cx| {
            window.dispatch_event(event, cx);
        })
        .expect("dispatch");
        draw(cx);
    };
    send(
        cx,
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: start,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
    );
    send(
        cx,
        PlatformInput::MouseMove(MouseMoveEvent {
            position: end,
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
        }),
    );
    send(
        cx,
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: end,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
    );
    let paragraph = &texts[1];
    let link = paragraph
        .text
        .find("https://github.com")
        .expect("link in the first paragraph");
    send(
        cx,
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at(1, link + 4),
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
    );
    let selected = cx.read_entity(view, |view, _| view.selected_text());
    eprintln!("selected: {selected:?}");
}

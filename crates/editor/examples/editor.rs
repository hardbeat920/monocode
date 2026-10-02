//! Opens a file in `CodeEditor` with its HEAD version as the git base.
//!
//! ```text
//! cargo run -p monocode-editor --example editor -- <path> [options]
//!   --light              light theme
//!   --line <n>           put the cursor on line n
//!   --find <text>        open the find bar with a query
//!   --replace <text>     also fill the replace field
//!   --regex              use a regular expression
//!   --peek <n>           open the removed-lines panel of hunk n (0-based)
//!   --nowrap             turn soft wrap off
//!   --readonly           open read-only
//!   --screenshot <png>   write what the window draws, then quit
//! ```
//!
//! Cmd-S writes the file back.

use std::{path::PathBuf, process::Command, time::Duration};

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Window, WindowBounds, WindowOptions, div, px, size,
};
use monocode_editor::{CodeEditor, EditorTheme, SaveRequest, search::SearchQuery};

struct Options {
    path: PathBuf,
    light: bool,
    line: Option<usize>,
    find: Option<String>,
    replace: Option<String>,
    regex: bool,
    peek: Option<usize>,
    nowrap: bool,
    readonly: bool,
    screenshot: Option<PathBuf>,
}

fn parse_options() -> Options {
    let mut args = std::env::args().skip(1);
    let mut options = Options {
        path: PathBuf::new(),
        light: false,
        line: None,
        find: None,
        replace: None,
        regex: false,
        peek: None,
        nowrap: false,
        readonly: false,
        screenshot: None,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--light" => options.light = true,
            "--line" => options.line = args.next().and_then(|v| v.parse().ok()),
            "--find" => options.find = args.next(),
            "--replace" => options.replace = args.next(),
            "--regex" => options.regex = true,
            "--peek" => options.peek = args.next().and_then(|v| v.parse().ok()),
            "--nowrap" => options.nowrap = true,
            "--readonly" => options.readonly = true,
            "--screenshot" => options.screenshot = args.next().map(PathBuf::from),
            _ => options.path = PathBuf::from(arg),
        }
    }
    if options.path.as_os_str().is_empty() {
        eprintln!("usage: editor <path> [--screenshot out.png]");
        std::process::exit(2);
    }
    options
}

/// `git show HEAD:<path>`, run from the file's directory.
fn head_text(path: &std::path::Path) -> Option<String> {
    let dir = path.parent().filter(|dir| !dir.as_os_str().is_empty())?;
    let name = path.file_name()?.to_str()?;
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .arg("show")
        .arg(format!("HEAD:./{name}"))
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

struct Shell {
    editor: Entity<CodeEditor>,
    theme: EditorTheme,
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(self.theme.panel_background)
            .child(self.editor.clone())
    }
}

fn main() {
    let options = parse_options();
    let path = std::fs::canonicalize(&options.path).unwrap_or(options.path.clone());
    let text = std::fs::read_to_string(&path).expect("read file");
    let head = head_text(&path);
    let theme = if options.light {
        EditorTheme::light()
    } else {
        EditorTheme::dark()
    };

    gpui_platform::application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        monocode_editor::init(cx);
        let bounds = Bounds::centered(None, size(px(1000.), px(720.)), cx);
        let screenshot = options.screenshot.clone();
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| {
                    let editor = cx.new(|cx| {
                        let mut editor = CodeEditor::new(
                            path.to_string_lossy().to_string(),
                            &text,
                            theme.clone(),
                            window,
                            cx,
                        );
                        editor.set_git_base(head.as_deref(), cx);
                        editor.set_footer(
                            path.file_name()
                                .map(|name| name.to_string_lossy().to_string().into()),
                            cx,
                        );
                        editor.on_save(std::rc::Rc::new(
                            |request: SaveRequest, _, cx: &mut App| {
                                cx.background_spawn(async move {
                                    std::fs::write(request.path.as_ref(), request.contents)?;
                                    Ok(())
                                })
                            },
                        ));
                        if options.readonly {
                            editor.set_read_only(true, cx);
                        }
                        if options.nowrap {
                            editor.set_soft_wrap(false, window, cx);
                        }
                        editor
                    });
                    editor.update(cx, |editor, cx| {
                        if let Some(line) = options.line {
                            editor.reveal_position(line, None, window, cx);
                        } else {
                            editor.focus(window, cx);
                        }
                    });
                    cx.new(|_| Shell { editor, theme })
                },
            )
            .expect("open window");

        let find = options.find.clone();
        let replace = options.replace.clone();
        let regex = options.regex;
        let peek = options.peek;
        cx.spawn(async move |cx| {
            // Let the first frames lay out and the git diff finish.
            cx.background_executor()
                .timer(Duration::from_millis(600))
                .await;
            let _ = window.update(cx, |shell, window, cx| {
                shell.editor.update(cx, |editor, cx| {
                    if let Some(search) = find {
                        let query = SearchQuery {
                            search,
                            replace: replace.unwrap_or_default(),
                            regexp: regex,
                            ..Default::default()
                        };
                        editor.set_find_query(query, window, cx);
                    }
                    if peek.is_some() {
                        editor.show_hunk(peek, cx);
                    }
                });
            });
            let Some(screenshot) = screenshot else {
                return;
            };
            cx.background_executor()
                .timer(Duration::from_millis(900))
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

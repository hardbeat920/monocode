//! Shows `git diff` for the repository in the current directory in `DiffView`.
//!
//! ```text
//! cargo run -p monocode-editor --example diff -- [options]
//!   --repo <dir>         diff this repository instead
//!   --staged             show `git diff --cached`
//!   --light              light theme
//!   --stage-first        stage the first hunk on start, to check the patch
//!   --screenshot <png>   write what the window draws, then quit
//! ```
//!
//! Hunk buttons run `git apply --cached` (stage), `git apply --cached
//! --reverse` (unstage), or `git apply --reverse` (discard), then reload.

use std::{path::PathBuf, process::Command, rc::Rc, time::Duration};

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Window, WindowBounds, WindowOptions, div, px, size,
};
use monocode_editor::{
    DiffAction, DiffFileActions, DiffView, EditorTheme, HunkActionRequest, InitialExpansion,
    parse_diff,
};

fn git(repo: &PathBuf, args: &[&str], input: Option<&str>) -> anyhow::Result<String> {
    use std::io::Write as _;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    if let Some(input) = input {
        child.stdin.take().unwrap().write_all(input.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn load(repo: &PathBuf, staged: bool) -> Vec<monocode_editor::DiffFile> {
    let mut args = vec!["diff", "--no-color", "--no-ext-diff"];
    if staged {
        args.push("--cached");
    }
    let text = git(repo, &args, None).unwrap_or_default();
    let actions = DiffFileActions {
        stage_hunk: !staged,
        discard_hunk: !staged,
        unstage_hunk: staged,
        comment: true,
        ..Default::default()
    };
    parse_diff(&text)
        .into_iter()
        .map(|file| file.with_actions(actions))
        .collect()
}

struct Shell {
    view: Entity<DiffView>,
    theme: EditorTheme,
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(self.theme.panel_background)
            .child(self.view.clone())
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut repo = std::env::current_dir().expect("current dir");
    let mut staged = false;
    let mut light = false;
    let mut screenshot: Option<PathBuf> = None;
    let mut stage_first = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--repo" => repo = args.next().map(PathBuf::from).expect("--repo <dir>"),
            "--staged" => staged = true,
            "--light" => light = true,
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            "--stage-first" => stage_first = true,
            other => eprintln!("unknown argument {other}"),
        }
    }
    let theme = if light {
        EditorTheme::light()
    } else {
        EditorTheme::dark()
    };

    gpui_platform::application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        monocode_editor::init(cx);
        let bounds = Bounds::centered(None, size(px(1000.), px(720.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_, cx| {
                    let files = load(&repo, staged);
                    let view = cx.new(|cx| DiffView::new(files, theme.clone(), cx));
                    let weak = view.downgrade();
                    let handler_repo = repo.clone();
                    view.update(cx, |view, _| {
                        view.on_hunk_action(Some(Rc::new(
                            move |request: HunkActionRequest, _, cx: &mut App| {
                                let args: &[&str] = match request.action {
                                    DiffAction::Stage => &["apply", "--cached", "-"],
                                    DiffAction::Unstage => &["apply", "--cached", "--reverse", "-"],
                                    DiffAction::Discard => &["apply", "--reverse", "-"],
                                };
                                if let Err(error) = git(&handler_repo, args, Some(&request.patch)) {
                                    eprintln!("git apply failed: {error}");
                                }
                                let files = load(&handler_repo, staged);
                                let _ = weak.update(cx, |view, cx| {
                                    view.set_files(files, InitialExpansion::All, cx)
                                });
                            },
                        )));
                        view.on_comment(Some(Rc::new(|target, _, _| {
                            eprintln!("comment on {}", target.location());
                        })));
                    });
                    cx.new(|_| Shell { view, theme })
                },
            )
            .expect("open window");

        cx.spawn(async move |cx| {
            if stage_first {
                let _ = window.update(cx, |shell, window, cx| {
                    shell.view.update(cx, |view, cx| {
                        view.run_hunk_action(0, 0, DiffAction::Stage, window, cx)
                    });
                });
            }
            let Some(screenshot) = screenshot else {
                return;
            };
            // Let the rows lay out and the background highlighting finish.
            cx.background_executor()
                .timer(Duration::from_millis(1500))
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

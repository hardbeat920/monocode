//! The composer gallery: one composer per scene, wired to a mock host.
//!
//! ```text
//! cargo run -p monocode-view-composer --example composer -j 4 -- [options]
//!   --scene <name>       empty, draft, busy, slash, mention, plus, edit, attach, meter
//!   --theme dark|light   color scheme (default dark)
//!   --size WxH           window size in points (default 640x360)
//!   --screenshot <png>   write what the window draws, then quit
//! ```

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use base64::Engine as _;
use gpui::{
    App, AppContext as _, Bounds, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Task, Window, WindowBounds, WindowOptions, div, px, size,
};
use monocode_core::attachment::{kind_from_mime, mime_from_name};
use monocode_core::context_usage::ContextUsage;
use monocode_core::models::ModelCatalog;
use monocode_core::session::QueuedMessage;
use monocode_core::{Attachment, HarnessId};
use monocode_ui::{AppearanceSettings, Theme, ThemePreference};
use monocode_view_composer::composer::model::chat_context::{ChatContextItem, DiffLineChange};
use monocode_view_composer::composer::model::clipboard::ClipboardFile;
use monocode_view_composer::composer::model::mcp::{McpConnection, McpTag};
use monocode_view_composer::composer::model::mentions::{ProjectFile, RankedFile};
use monocode_view_composer::composer::model::skills::Skill;
use monocode_view_composer::composer::{
    Composer, ComposerHost, ComposerProps, ComposerSubmission, LastTurnRecall, SessionFolder,
    SkillContext,
};
use monocode_view_composer::pickers::{LocalModelSource, ModelSource};

struct Options {
    scene: String,
    light: bool,
    size: (f32, f32),
    screenshot: Option<PathBuf>,
}

fn parse_options() -> Options {
    let mut args = std::env::args().skip(1);
    let mut options = Options {
        scene: "draft".into(),
        light: false,
        size: (640., 360.),
        screenshot: None,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scene" => options.scene = args.next().unwrap_or_default(),
            "--theme" => options.light = args.next().as_deref() == Some("light"),
            "--size" => {
                if let Some((w, h)) = args.next().as_deref().and_then(|v| v.split_once('x')) {
                    options.size = (w.parse().unwrap_or(640.), h.parse().unwrap_or(360.));
                }
            }
            "--screenshot" => options.screenshot = args.next().map(PathBuf::from),
            other => eprintln!("ignoring {other}"),
        }
    }
    options
}

/// A host with canned skills, files, and MCP servers.
struct MockHost;

fn files() -> Vec<ProjectFile> {
    vec![
        ProjectFile::new(
            "App.tsx",
            "/Users/me/code/agent-terminal/src/App.tsx",
            "src/App.tsx",
        ),
        ProjectFile::new(
            "gridArcade.ts",
            "/Users/me/code/agent-terminal/src/surfaces/gridArcade.ts",
            "src/surfaces/gridArcade.ts",
        ),
        ProjectFile::new(
            "README.md",
            "/Users/me/code/agent-terminal/README.md",
            "README.md",
        ),
        ProjectFile::new(
            "Cargo.toml",
            "/Users/me/code/agent-terminal/Cargo.toml",
            "Cargo.toml",
        ),
    ]
}

impl ComposerHost for MockHost {
    fn submit(&self, submission: ComposerSubmission, _: &mut Window, _: &mut App) -> bool {
        eprintln!("submit: {:?}", submission.text);
        true
    }

    fn attachments_from_paths(&self, paths: Vec<String>, _: &mut App) -> Task<Vec<Attachment>> {
        Task::ready(
            paths
                .into_iter()
                .map(|path| {
                    let name = path.rsplit('/').next().unwrap_or(&path).to_string();
                    let mime = mime_from_name(&name);
                    Attachment {
                        id: path.clone(),
                        kind: kind_from_mime(&mime),
                        mime_type: mime,
                        name,
                        path: Some(path),
                        ..Attachment::default()
                    }
                })
                .collect(),
        )
    }

    fn attachments_from_files(
        &self,
        files: Vec<ClipboardFile>,
        _: &mut App,
    ) -> Task<Vec<Attachment>> {
        Task::ready(
            files
                .into_iter()
                .enumerate()
                .map(|(index, file)| Attachment {
                    id: format!("pasted-{index}"),
                    kind: kind_from_mime(&file.mime_type),
                    data: Some(base64::engine::general_purpose::STANDARD.encode(&file.bytes)),
                    size: file.bytes.len() as i64,
                    mime_type: file.mime_type,
                    name: file.name,
                    ..Attachment::default()
                })
                .collect(),
        )
    }

    fn pick_attachments(&self, _: &mut Window, _: &mut App) -> Task<Vec<Attachment>> {
        Task::ready(Vec::new())
    }

    fn skills(&self, _: &SkillContext, _: &mut App) -> Vec<Skill> {
        vec![
            Skill::file(
                "review-pr",
                "Review pull requests against team standards.",
                "/repo/.agents/skills/review-pr/SKILL.md",
                "project",
                "agents",
            ),
            Skill::file(
                "release-notes",
                "Draft release notes from merged pull requests.",
                "/repo/.agents/skills/release-notes/SKILL.md",
                "project",
                "agents",
            ),
            Skill::file(
                "triage",
                "Sort new issues by area and urgency.",
                "/Users/me/.agents/skills/triage/SKILL.md",
                "user",
                "agents",
            ),
        ]
    }

    fn mention_files(&self, _: &str, _: &mut App) -> Vec<ProjectFile> {
        files()
    }

    fn rank_mentions(&self, _: &str, query: &str, _: &mut App) -> Vec<RankedFile> {
        files()
            .into_iter()
            .filter(|file| file.relative.to_lowercase().contains(&query.to_lowercase()))
            .map(|file| RankedFile {
                file,
                score: 0,
                positions: Vec::new(),
            })
            .collect()
    }

    fn session_folders(&self, _: &str, _: &mut App) -> Vec<SessionFolder> {
        vec![SessionFolder {
            id: "f1".into(),
            name: "Arcade".into(),
            session_count: 3,
        }]
    }

    fn model_source(&self, _: &mut App) -> Option<Rc<dyn ModelSource>> {
        Some(Rc::new(LocalModelSource::new(
            ModelCatalog::new(),
            monocode_view_composer::pickers::model_source::all_available(),
        )))
    }
}

fn docs_tag() -> McpTag {
    McpTag {
        server: McpConnection {
            provider: "claude".into(),
            name: "docs".into(),
            scope: "project".into(),
            config_path: "/repo/.mcp.json".into(),
            transport: "stdio".into(),
            enabled: None,
        },
        token: "@mcp/docs".into(),
    }
}

/// A 2x2 checker PNG for the image chip.
fn sample_png() -> String {
    let mut image = image::RgbaImage::new(24, 24);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let warm = (x / 6 + y / 6) % 2 == 0;
        *pixel = if warm {
            image::Rgba([232, 185, 35, 255])
        } else {
            image::Rgba([56, 189, 248, 255])
        };
    }
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encode png");
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn attachments() -> Vec<Attachment> {
    vec![
        Attachment {
            id: "img".into(),
            name: "screenshot.png".into(),
            mime_type: "image/png".into(),
            kind: monocode_core::AttachmentKind::Image,
            size: 1200,
            data: Some(sample_png()),
            ..Attachment::default()
        },
        Attachment {
            id: "pdf".into(),
            name: "design-review.pdf".into(),
            mime_type: "application/pdf".into(),
            kind: monocode_core::AttachmentKind::File,
            size: 40_000,
            path: Some("/Users/me/Downloads/design-review.pdf".into()),
            ..Attachment::default()
        },
    ]
}

struct Gallery {
    composer: Entity<Composer>,
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_end()
            .bg(theme.colors.background_base)
            .text_color(theme.colors.content)
            .child(div().flex_1())
            .child(div().w_full().child(self.composer.clone()))
    }
}

fn props_for(scene: &str) -> ComposerProps {
    let mut props = ComposerProps {
        harness: HarnessId::Cursor,
        model: "cursor:grok-4.6".into(),
        cwd: "/Users/me/code/agent-terminal".into(),
        execution_cwd: "/Users/me/code/agent-terminal".into(),
        session_id: Some("s1".into()),
        branch: Some("main".into()),
        model_controls_beside: true,
        can_save_draft: true,
        btw_enabled: true,
        folders_enabled: true,
        compact_supported: true,
        animate: false,
        focused: true,
        context: Some(ContextUsage {
            used: 118_000,
            window: Some(200_000),
        }),
        ..ComposerProps::default()
    };
    props
        .model_settings
        .insert("reasoning".into(), "high".into());
    match scene {
        "busy" => {
            props.busy = true;
            props.queued_messages = vec![QueuedMessage {
                id: "q1".into(),
                text: "Then run the arcade tests again".into(),
                attachments: Vec::new(),
                note_card: None,
                handoff_card: None,
                intent: None,
            }];
        }
        "edit" => {
            props.edit_last_turn_supported = true;
            props.last_turn_recall = Some(LastTurnRecall {
                text: "no that was not a good change actually revert back to the previous version"
                    .into(),
                attachments: Vec::new(),
            });
        }
        _ => {}
    }
    props
}

fn setup(scene: &str, composer: &Entity<Composer>, window: &mut Window, cx: &mut App) {
    composer.update(cx, |composer, cx| {
        composer.focus(window, cx);
        match scene {
            "draft" => {
                composer.set_selected_mcp(vec![docs_tag()], cx);
                composer.set_text(
                    "/plan fix the win screen in @src/surfaces/gridArcade.ts with /review-pr and @mcp/docs ",
                    cx,
                );
            }
            "slash" => composer.set_text("/re", cx),
            "mention" => composer.set_text("look at @src", cx),
            "plus" => composer.set_plus_open(true, cx),
            "busy" => composer.set_text("also check the pong paddle speed", cx),
            "edit" => composer.recall_last_turn(window, cx),
            "attach" => {
                composer.set_context_items(
                    vec![
                        ChatContextItem::Code {
                            path: "src/surfaces/gridArcade.ts".into(),
                            start_line: 84,
                            end_line: 99,
                        },
                        ChatContextItem::Quote {
                            text: "Each game now plays all the way through instead of cutting off mid-move."
                                .into(),
                        },
                        ChatContextItem::Comment {
                            path: "src/App.tsx".into(),
                            line: Some(42),
                            change: DiffLineChange::Added,
                            code: "const arcade = useArcade();".into(),
                            comment: "Move this into the hook".into(),
                        },
                    ],
                    cx,
                );
                composer.add_attachments(attachments(), window, cx);
                composer.set_text("Why does this flicker?", cx);
            }
            "meter" => composer.set_context_details_open(true, cx),
            _ => {}
        }
    });
}

fn main() {
    let options = parse_options();
    gpui_platform::application()
        .with_assets(monocode_ui::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            let appearance = AppearanceSettings {
                theme_preference: ThemePreference::parse(Some(if options.light {
                    "light"
                } else {
                    "dark"
                })),
                ..AppearanceSettings::default()
            };
            monocode_ui::init(appearance, cx);
            monocode_view_composer::composer::init(cx);
            monocode_view_composer::pickers::init(cx);
            let bounds = Bounds::centered(None, size(px(options.size.0), px(options.size.1)), cx);
            let scene = options.scene.clone();
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        ..Default::default()
                    },
                    |window, cx| {
                        monocode_ui::sync_window(window, cx);
                        let host: Rc<dyn ComposerHost> = Rc::new(MockHost);
                        let props = props_for(&scene);
                        let composer = cx.new(|cx| Composer::new(host, props, None, window, cx));
                        setup(&scene, &composer, window, cx);
                        cx.new(|_| Gallery { composer })
                    },
                )
                .expect("open window");
            // A screenshot run stays in the background so it never takes
            // the keyboard from whatever app is in front.
            let Some(screenshot) = options.screenshot.clone() else {
                cx.activate(true);
                return;
            };
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(1200))
                    .await;
                let any: gpui::AnyWindowHandle = window.into();
                let result = any.update(cx, |_, window, cx| {
                    window.refresh();
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

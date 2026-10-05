//! Shows the transcript cards with synthetic data.
//!
//! ```sh
//! cargo run -p monocode-view-transcript --example cards_gallery -- --card question
//! cargo run -p monocode-view-transcript --example cards_gallery -- \
//!     --card link --screenshot target/cards-shots/link.png
//! ```
//!
//! Cards: link, link-loading, question, outline, find, diff, toasts,
//! selection, plan, tasks, note, image, lightbox, markdown, spinner, bursts.
//! `--theme light` switches the scheme, `--burst-ms N` picks the burst frame.
//! `--screenshot` draws the window offscreen with `Window::render_to_image`
//! and exits. A screenshot window is never shown or focused, so it cannot
//! take the keyboard from the user.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AnyView, App, AppContext as _, AsyncApp, Bounds, Context, Entity, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Window, WindowBounds, WindowOptions,
    div, point, px, size,
};
use gpui_component::Root;
use monocode_core::block::{
    GeneratedImageMeta, PlanStatus, TaskListItem, TaskListItemStatus, ToolPreview, ToolPreviewKind,
    ToolPreviewLine, ToolPreviewLineKind,
};
use monocode_core::notes::NoteCardMeta;
use monocode_core::transcript::ToolCallState;
use monocode_core::user_question::{UserQuestion, UserQuestionOption, UserQuestionPrompt};
use monocode_core::{Block, BlockRole, HarnessId};
use monocode_ui::styled::UiStyled as _;
use monocode_ui::{AppearanceSettings, Theme, ThemePreference, file_type_icon, u};
use monocode_view_transcript::cards::approval_toasts::{
    ApprovalNotice, ApprovalToasts, NoticeKind,
};
use monocode_view_transcript::cards::celebration::{BurstKind, celebration_burst};
use monocode_view_transcript::cards::find_bar::TranscriptFind;
use monocode_view_transcript::cards::generated_image::GeneratedImage;
use monocode_view_transcript::cards::link_preview::{
    LinkPreviewMetadata, LinkPreviews, LinkWorkItem, LinkWorkItemDetails, UserLinkPreview,
    WorkItemLabel, WorkItemPerson, work_item_key,
};
use monocode_view_transcript::cards::markdown_document::MarkdownDocumentPreview;
use monocode_view_transcript::cards::markdown_mode::{MarkdownViewMode, markdown_view_shell};
use monocode_view_transcript::cards::note_card::{NoteProject, note_card};
use monocode_view_transcript::cards::plan_preview::plan_preview;
use monocode_view_transcript::cards::prompt_outline::PromptOutline;
use monocode_view_transcript::cards::prompt_outline_model::{OutlineAnchor, OutlineBand};
use monocode_view_transcript::cards::question_form::QuestionForm;
use monocode_view_transcript::cards::selection_menu::{
    SelectionAction, TranscriptSelection, TranscriptSelectionMenu,
};
use monocode_view_transcript::cards::spinner::terminal_spinner;
use monocode_view_transcript::cards::task_list::task_list_preview;
use monocode_view_transcript::cards::tool_diff::ToolDiffPreview;
use monocode_view_transcript::transcript::model::link::parse_user_message_link;

const USAGE: &str = "\
usage: cards_gallery [--card <name>] [--theme dark|light] [--size WxH]
                     [--burst-ms <n>] [--screenshot <out.png>]";

#[derive(Debug, Clone)]
struct Args {
    card: String,
    light: bool,
    size: Option<(f32, f32)>,
    burst_ms: u64,
    screenshot: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        card: "question".into(),
        light: false,
        size: None,
        burst_ms: 900,
        screenshot: None,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| iter.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--card" => args.card = value("--card")?,
            "--theme" => args.light = value("--theme")? == "light",
            "--burst-ms" => {
                args.burst_ms = value("--burst-ms")?
                    .parse()
                    .map_err(|_| "--burst-ms takes a number")?
            }
            "--size" => {
                let raw = value("--size")?;
                let (w, h) = raw.split_once('x').ok_or("--size takes WxH")?;
                args.size = Some((
                    w.parse().map_err(|_| "--size width")?,
                    h.parse().map_err(|_| "--size height")?,
                ));
            }
            "--screenshot" => args.screenshot = Some(PathBuf::from(value("--screenshot")?)),
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown flag {other}\n{USAGE}")),
        }
    }
    Ok(args)
}

fn default_size(card: &str) -> (f32, f32) {
    match card {
        "outline" => (1000., 560.),
        "toasts" => (720., 420.),
        "lightbox" => (900., 640.),
        "image" => (720., 560.),
        "markdown" => (760., 520.),
        "link" | "link-loading" => (720., 520.),
        "find" => (720., 260.),
        "diff" => (720., 420.),
        "selection" => (620., 300.),
        "bursts" => (720., 360.),
        "question" => (640., 560.),
        _ => (720., 560.),
    }
}

/// Elements drawn fresh each frame, after the card views.
type Elements = Box<dyn Fn(&mut Window, &mut App) -> Vec<AnyElement>>;

/// The page every card sits on: the transcript background and a caption.
struct Gallery {
    title: SharedString,
    content: Vec<AnyView>,
    elements: Elements,
    padded: bool,
}

impl Render for Gallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let elements = (self.elements)(window, cx);
        div()
            .size_full()
            .relative()
            .bg(theme.colors.background_base)
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone())
            .text_size(u(13.))
            .line_height(u(20.))
            .when(self.padded, |el| el.p(u(24.)))
            .child(
                div()
                    .mb(u(12.))
                    .font_family(theme.fonts.mono.clone())
                    .text_px(11.)
                    .text_color(theme.content(0.4))
                    .when(!self.padded, |el| el.absolute().top(u(8.)).left(u(12.)))
                    .child(self.title.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(u(16.))
                    .when(!self.padded, |el| el.size_full())
                    .children(
                        self.content
                            .iter()
                            .cloned()
                            .map(|view| view.into_any_element()),
                    )
                    .children(elements),
            )
    }
}

fn caption(text: &str, theme: &Theme) -> AnyElement {
    div()
        .font_family(theme.fonts.mono.clone())
        .text_px(10.)
        .text_color(theme.content(0.35))
        .child(text.to_string())
        .into_any_element()
}

fn page(
    title: &str,
    content: Vec<AnyView>,
    elements: impl Fn(&mut Window, &mut App) -> Vec<AnyElement> + 'static,
    padded: bool,
) -> Gallery {
    Gallery {
        title: title.to_string().into(),
        content,
        elements: Box::new(elements),
        padded,
    }
}

fn github_link(number: i64) -> monocode_view_transcript::transcript::model::link::UserLink {
    parse_user_message_link(&format!("https://github.com/acme/widgets/pull/{number}"))
        .unwrap()
        .link
}

fn seed_link_cache(cx: &mut App) {
    LinkPreviews::set_loader(cx, |_, _| {});
    let key = work_item_key(github_link(73).github_work_item.as_ref().unwrap());
    LinkPreviews::resolve_work_item(
        &key,
        Ok(LinkWorkItem {
            title: "Make work item links easier to scan".into(),
            state: "open".into(),
            draft: false,
            updated: Some("2 hours ago".into()),
            labels: vec![
                WorkItemLabel {
                    name: "enhancement".into(),
                    color: "8b5cf6".into(),
                },
                WorkItemLabel {
                    name: "ui".into(),
                    color: "0e8a16".into(),
                },
                WorkItemLabel {
                    name: "transcript".into(),
                    color: "d93f0b".into(),
                },
                WorkItemLabel {
                    name: "good first issue".into(),
                    color: "7057ff".into(),
                },
            ],
            assignees: vec![
                WorkItemPerson {
                    login: "grace".into(),
                    avatar_url: None,
                },
                WorkItemPerson {
                    login: "linus".into(),
                    avatar_url: None,
                },
            ],
        }),
        Ok(LinkWorkItemDetails {
            body: "Adds **compact chips** and a useful hover preview. The card shows the state, the branch, and who is on it, so a link in a prompt reads at a glance.".into(),
            author: "ada".into(),
            author_avatar_url: None,
            base_ref_name: Some("main".into()),
            head_ref_name: Some("link-chips".into()),
        }),
        cx,
    );
    let favicon = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(include_bytes!("../assets/monocode.png"))
    );
    LinkPreviews::resolve_metadata(
        "https://docs.rs/gpui/latest/gpui/",
        Ok(LinkPreviewMetadata {
            title: Some("gpui - Rust".into()),
            favicon_data_url: Some(favicon),
        }),
        cx,
    );
}

fn edit_preview() -> ToolPreview {
    let mut preview = ToolPreview::new(ToolPreviewKind::Write);
    preview.path = Some("/Users/dev/arcade/src/surfaces/speechBubble.ts".into());
    preview.additions = Some(3);
    preview.deletions = Some(2);
    let line = |kind, text: &str, number| ToolPreviewLine {
        kind,
        text: text.into(),
        number: Some(number),
        extra: Default::default(),
    };
    preview.lines = Some(vec![
        line(
            ToolPreviewLineKind::Context,
            "export function bubbleWidth(text: string) {",
            12,
        ),
        line(
            ToolPreviewLineKind::Del,
            "  const width = text.length * 7;",
            13,
        ),
        line(
            ToolPreviewLineKind::Del,
            "  return Math.min(width, 320);",
            14,
        ),
        line(
            ToolPreviewLineKind::Add,
            "  // Measure with the real font instead of a guess.",
            13,
        ),
        line(
            ToolPreviewLineKind::Add,
            "  const width = measureText(text, BubbleFont);",
            14,
        ),
        line(
            ToolPreviewLineKind::Add,
            "  return Math.min(width, MAX_BUBBLE_WIDTH);",
            15,
        ),
    ]);
    preview
}

fn build(args: &Args, window: &mut Window, cx: &mut App) -> Gallery {
    match args.card.as_str() {
        "link" | "link-loading" => {
            seed_link_cache(cx);
            let loading = args.card == "link-loading";
            let number = if loading { 81 } else { 73 };
            let chip = cx.new(|cx| {
                let mut view = UserLinkPreview::new(
                    github_link(number),
                    Some("/Users/dev/widgets".into()),
                    true,
                    window,
                    cx,
                );
                view.show_now(cx);
                view
            });
            let issue = cx.new(|cx| {
                UserLinkPreview::new(
                    parse_user_message_link("https://github.com/acme/widgets/issues/42")
                        .unwrap()
                        .link,
                    None,
                    false,
                    window,
                    cx,
                )
            });
            let generic = cx.new(|cx| {
                UserLinkPreview::new(
                    parse_user_message_link("https://docs.rs/gpui/latest/gpui/")
                        .unwrap()
                        .link,
                    None,
                    true,
                    window,
                    cx,
                )
            });
            let row = move |label: &str, view: AnyView, theme: &Theme| {
                div()
                    .flex()
                    .items_center()
                    .gap(u(12.))
                    .child(div().w(u(140.)).child(caption(label, theme)))
                    .child(view)
                    .into_any_element()
            };
            let (chip, issue, generic): (AnyView, AnyView, AnyView) =
                (chip.into(), issue.into(), generic.into());
            page(
                "UserLinkPreview",
                Vec::new(),
                move |_, cx| {
                    let theme = Theme::of(cx).clone();
                    vec![
                        div().h(u(300.)).into_any_element(),
                        row("compact PR chip", chip.clone(), &theme),
                        row("issue chip", issue.clone(), &theme),
                        row("generic link", generic.clone(), &theme),
                    ]
                },
                true,
            )
        }
        "question" => {
            let prompt = UserQuestionPrompt {
                request_id: 4,
                title: Some("Release plan".into()),
                auto_resolve_at: Some(monocode_view_transcript::cards::util::now_ms() + 42_000),
                questions: vec![
                    UserQuestion {
                        id: "targets".into(),
                        header: Some("Targets".into()),
                        prompt: "Which platforms should this release ship to?".into(),
                        multi_select: true,
                        allow_custom: true,
                        options: vec![
                            UserQuestionOption {
                                id: "mac".into(),
                                label: "macOS".into(),
                                description: Some("Signed and notarized dmg".into()),
                            },
                            UserQuestionOption {
                                id: "linux".into(),
                                label: "Linux".into(),
                                description: Some("deb, rpm, and AppImage".into()),
                            },
                            UserQuestionOption {
                                id: "windows".into(),
                                label: "Windows".into(),
                                description: None,
                            },
                        ],
                    },
                    UserQuestion {
                        id: "channel".into(),
                        header: None,
                        prompt: "Which channel?".into(),
                        multi_select: false,
                        allow_custom: false,
                        options: vec![UserQuestionOption {
                            id: "beta".into(),
                            label: "Beta".into(),
                            description: None,
                        }],
                    },
                ],
            };
            let form = cx.new(|cx| {
                let mut form = QuestionForm::new(prompt, window, cx);
                form.select("mac", cx);
                form.select(monocode_core::user_question::CUSTOM_OPTION_ID, cx);
                form.highlight(1, window, cx);
                form
            });
            let single = cx.new(|cx| {
                let mut form = QuestionForm::new(
                    UserQuestionPrompt {
                        request_id: 9,
                        title: None,
                        auto_resolve_at: None,
                        questions: vec![UserQuestion {
                            id: "colour".into(),
                            header: None,
                            prompt: "Pick a colour".into(),
                            multi_select: false,
                            allow_custom: false,
                            options: ["Red", "Green", "Blue"]
                                .iter()
                                .map(|label| UserQuestionOption {
                                    id: label.to_lowercase(),
                                    label: label.to_string(),
                                    description: None,
                                })
                                .collect(),
                        }],
                    },
                    window,
                    cx,
                );
                form.select("green", cx);
                form
            });
            page(
                "QuestionForm",
                vec![form.into(), single.into()],
                |_, _| Vec::new(),
                true,
            )
        }
        "outline" => {
            let blocks: Vec<Arc<Block>> = (1..=8)
                .flat_map(|turn| {
                    [
                        Arc::new(Block::new(
                            format!("u{turn}"),
                            BlockRole::User,
                            match turn {
                                5 => "Why does the speech bubble wrap early on narrow panes?".to_string(),
                                _ => format!("Prompt number {turn} about the arcade"),
                            },
                        )),
                        Arc::new(Block::new(
                            format!("a{turn}"),
                            BlockRole::Assistant,
                            "The bubble measured text with a guessed glyph width.\nI switched it to the real font metrics and capped it at the pane width.",
                        )),
                    ]
                })
                .collect();
            let outline = cx.new(|cx| {
                let mut outline = PromptOutline::new(cx);
                outline.set_blocks(blocks, cx);
                let anchors: Vec<OutlineAnchor> = (1..=8)
                    .map(|turn| OutlineAnchor {
                        id: format!("u{turn}"),
                        top: turn as f32 * 200.,
                        bottom: turn as f32 * 200. + 40.,
                    })
                    .collect();
                outline.set_viewport(
                    OutlineBand {
                        top: 550.,
                        bottom: 1100.,
                    },
                    &anchors,
                    500.,
                    cx,
                );
                outline.show_bar("u5", cx);
                outline
            });
            let outline: AnyView = outline.into();
            page(
                "PromptOutline",
                Vec::new(),
                move |_, _| {
                    vec![
                        div()
                            .relative()
                            .size_full()
                            .child(outline.clone())
                            .into_any_element(),
                    ]
                },
                false,
            )
        }
        "find" => {
            let blocks: Vec<Arc<Block>> =
                ["cache the font", "a cache miss", "no hit", "cache again"]
                    .iter()
                    .enumerate()
                    .map(|(index, text)| {
                        Arc::new(Block::new(format!("b{index}"), BlockRole::Assistant, *text))
                    })
                    .collect();
            let find = cx.new(|cx| {
                let mut find = TranscriptFind::new(window, cx);
                find.set_blocks(blocks, cx);
                find.open(cx);
                find.search("cache", window, cx);
                find.step(1, cx);
                find
            });
            let empty = cx.new(|cx| {
                let mut find = TranscriptFind::new(window, cx);
                find.set_side(
                    monocode_view_transcript::cards::find_bar::FindSide::Left,
                    cx,
                );
                find.open(cx);
                find.search("zebra", window, cx);
                find
            });
            let (find, empty): (AnyView, AnyView) = (find.into(), empty.into());
            page(
                "TranscriptFind",
                Vec::new(),
                move |_, _| {
                    vec![
                        div()
                            .relative()
                            .h(u(80.))
                            .w_full()
                            .child(find.clone())
                            .into_any_element(),
                        div()
                            .relative()
                            .h(u(80.))
                            .w_full()
                            .child(empty.clone())
                            .into_any_element(),
                    ]
                },
                true,
            )
        }
        "diff" => {
            let preview = cx.new(|cx| {
                let mut preview = ToolDiffPreview::new(
                    edit_preview(),
                    "speechBubble.ts",
                    ToolCallState::Accepted,
                    Some("/Users/dev/arcade".into()),
                    true,
                    |_, cx| {
                        let theme = Theme::of(cx).clone();
                        div()
                            .flex()
                            .items_center()
                            .gap(u(4.))
                            .px(u(4.))
                            .rounded(u(4.))
                            .font_family(theme.fonts.mono.clone())
                            .text_px(13.)
                            .text_color(theme.content(0.85))
                            .child(file_type_icon("speechBubble.ts"))
                            .child("src/surfaces/speechBubble.ts")
                            .into_any_element()
                    },
                    window,
                    cx,
                );
                preview.show(cx);
                preview
            });
            let preview: AnyView = preview.into();
            page(
                "ToolDiffPreview",
                Vec::new(),
                move |_, cx| {
                    let theme = Theme::of(cx).clone();
                    vec![
                        div()
                            .flex()
                            .items_center()
                            .gap(u(6.))
                            .font_family(theme.fonts.sans.clone())
                            .text_px(14.)
                            .text_color(theme.content(0.5))
                            .child("Edit")
                            .child(preview.clone())
                            .into_any_element(),
                    ]
                },
                true,
            )
        }
        "toasts" => {
            let toasts = cx.new(|cx| {
                let mut toasts = ApprovalToasts::new(cx);
                toasts.set_notices(
                    vec![
                        ApprovalNotice {
                            session_id: "s1".into(),
                            request_id: 3,
                            label: "Run cargo test -p monocode-view-transcript -j 4".into(),
                            kind: NoticeKind::Approval,
                            session_title: "Port the transcript cards".into(),
                            harness: HarnessId::Claude,
                            cwd: "/Users/dev/monocode".into(),
                        },
                        ApprovalNotice {
                            session_id: "s2".into(),
                            request_id: 5,
                            label: "Which branch should the release cut from?".into(),
                            kind: NoticeKind::Question,
                            session_title: "Release checklist".into(),
                            harness: HarnessId::Codex,
                            cwd: "/Users/dev/monocode".into(),
                        },
                    ],
                    cx,
                );
                toasts
            });
            page(
                "ApprovalToasts",
                vec![toasts.into()],
                |_, _| Vec::new(),
                false,
            )
        }
        "selection" => {
            let menu = cx.new(|cx| {
                let mut menu = TranscriptSelectionMenu::new(true, true);
                menu.set_selection(
                    Some(TranscriptSelection {
                        text: "measure with the real font".into(),
                        rect: Bounds::new(point(px(180.), px(170.)), size(px(220.), px(20.))),
                    }),
                    cx,
                );
                menu.pick(SelectionAction::AddToNotes, cx);
                menu.finish_note(Err("The notes folder is read-only.".into()), cx);
                menu
            });
            let menu: AnyView = menu.into();
            page(
                "TranscriptSelectionMenu",
                Vec::new(),
                move |_, cx| {
                    let theme = Theme::of(cx).clone();
                    vec![
                        div()
                            .absolute()
                            .top(px(170.))
                            .left(px(120.))
                            .text_px(14.)
                            .child(
                                gpui::StyledText::new(
                                    "We should measure with the real font instead.",
                                )
                                .with_highlights([(
                                    10..36,
                                    gpui::HighlightStyle {
                                        background_color: Some(theme.accent(0.35)),
                                        ..Default::default()
                                    },
                                )]),
                            )
                            .into_any_element(),
                        menu.clone().into_any_element(),
                    ]
                },
                false,
            )
        }
        "plan" => page(
            "PlanPreview",
            Vec::new(),
            |_, cx| {
                let theme = Theme::of(cx).clone();
                let text = "# Measure bubbles with the real font\n\nThe speech bubble guesses glyph widths, so long words wrap early on narrow panes. Measure with the text system and cap at the pane width.\n\n1. Add measureText\n2. Use it in bubbleWidth";
                vec![
                    caption("ready, with a build target", &theme),
                    plan_preview("plan-ready", text)
                        .status(Some(PlanStatus::Ready))
                        .on_open(|_, _, _| {})
                        .on_build(|_, _, _| {})
                        .on_pick_target(|_, _, _| {})
                        .into_any_element(),
                    caption("building", &theme),
                    plan_preview("plan-building", text)
                        .status(Some(PlanStatus::Building))
                        .on_open(|_, _, _| {})
                        .on_build(|_, _, _| {})
                        .into_any_element(),
                    caption("streaming, read-only", &theme),
                    plan_preview("plan-streaming", "# Draft plan\n\nStill writing")
                        .streaming(true)
                        .into_any_element(),
                ]
            },
            true,
        ),
        "tasks" => page(
            "TaskListPreview",
            Vec::new(),
            |_, _| {
                let item = |text: &str, status| TaskListItem {
                    id: None,
                    text: text.into(),
                    status,
                    extra: Default::default(),
                };
                vec![
                    task_list_preview(
                        "tasks",
                        vec![
                            item(
                                "Read the React sources and tests",
                                TaskListItemStatus::Completed,
                            ),
                            item("Port the question form", TaskListItemStatus::Completed),
                            item("Port the link preview card", TaskListItemStatus::InProgress),
                            item(
                                "Render the gallery screenshots",
                                TaskListItemStatus::Pending,
                            ),
                            item(
                                "Swap the old orchestration stand-in",
                                TaskListItemStatus::Cancelled,
                            ),
                        ],
                    )
                    .explanation(Some("Cards first, then the transcript swaps."))
                    .into_any_element(),
                ]
            },
            true,
        ),
        "note" => page(
            "NoteMiniCard",
            Vec::new(),
            |_, cx| {
                let theme = Theme::of(cx).clone();
                let meta = NoteCardMeta {
                    id: "n1".into(),
                    slug: "release-plan".into(),
                    title: "Release plan for the native app".into(),
                    source_cwd: Some("/Users/dev/monocode".into()),
                    extra: Default::default(),
                };
                vec![
                    caption("embedded in a prompt bubble", &theme),
                    div()
                        .w(u(420.))
                        .rounded(u(12.))
                        .bg(theme.content(0.1))
                        .px(u(12.))
                        .py(u(8.))
                        .child(
                            div()
                                .mb(u(8.))
                                .child(note_card("note-embedded", meta.clone()).embedded(true)),
                        )
                        .child(div().text_px(14.).child("Turn this into a checklist"))
                        .into_any_element(),
                    caption("in the composer, with a project and remove", &theme),
                    div()
                        .w(u(420.))
                        .child(
                            note_card("note-composer", meta)
                                .project(Some(NoteProject {
                                    name: "monocode".into(),
                                    logo: None,
                                    color: Some(gpui::rgb(0x7dd3fc).into()),
                                }))
                                .on_dismiss(|_, _, _| {}),
                        )
                        .into_any_element(),
                ]
            },
            true,
        ),
        "image" | "lightbox" => {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/monocode.png");
            let size = std::fs::metadata(&path)
                .map(|meta| meta.len() as i64)
                .unwrap_or(0);
            let open = args.card == "lightbox";
            let image = cx.new(|cx| {
                GeneratedImage::new(
                    GeneratedImageMeta {
                        path: path.to_string_lossy().into_owned(),
                        name: "monocode.png".into(),
                        mime_type: "image/png".into(),
                        size,
                        alt: Some("The MonoCode mark".into()),
                        extra: Default::default(),
                    },
                    cx,
                )
            });
            let missing = cx.new(|cx| {
                GeneratedImage::new(
                    GeneratedImageMeta {
                        path: "/nonexistent/render.png".into(),
                        name: "render.png".into(),
                        mime_type: "image/png".into(),
                        size: 0,
                        alt: None,
                        extra: Default::default(),
                    },
                    cx,
                )
            });
            if open {
                let image = image.clone();
                window
                    .spawn(cx, async move |cx| {
                        cx.background_executor()
                            .timer(Duration::from_millis(300))
                            .await;
                        image
                            .update_in(cx, |image, window, cx| image.open(window, cx))
                            .ok();
                    })
                    .detach();
            }
            page(
                "GeneratedImage",
                vec![image.into(), missing.into()],
                |_, _| Vec::new(),
                true,
            )
        }
        "markdown" => {
            let document = cx.new(|cx| {
                let mut preview = MarkdownDocumentPreview::new(
                    "---\ntitle: Release plan\ntags:\n  - native\n  - gpui\n---\n\n# Release plan\n\nShip the **native app** once every view has parity.\n\n- Cards\n- Threads\n- Settings\n",
                    "Properties",
                    cx,
                );
                preview.toggle_metadata(cx);
                preview
            });
            let document: AnyView = document.into();
            page(
                "MarkdownViewShell, MarkdownModeToggle, MarkdownDocumentPreview",
                Vec::new(),
                move |_, cx| {
                    let theme = Theme::of(cx).clone();
                    vec![
                        div()
                            .relative()
                            .h(u(440.))
                            .w_full()
                            .rounded(u(8.))
                            .border_1()
                            .border_color(theme.content(0.1))
                            .overflow_hidden()
                            .child(markdown_view_shell(
                                "markdown-mode",
                                MarkdownViewMode::Preview,
                                document.clone(),
                                div().p(u(16.)).child("source"),
                            ))
                            .into_any_element(),
                    ]
                },
                true,
            )
        }
        "transcript-orchestration" => {
            use monocode_core::orchestration::{
                OrchestrationChoice, OrchestrationProposal, OrchestrationProposalStatus,
                OrchestrationSettings, ProposedTask, proposal_block,
            };
            let choice = |harness: HarnessId, model: &str, name: &str| OrchestrationChoice {
                harness,
                model: model.into(),
                name: name.into(),
                extra: Default::default(),
            };
            let task = |id: &str, title: &str, harness, model: &str| ProposedTask {
                id: id.into(),
                title: title.into(),
                prompt: format!("{title}. Keep the change small and add tests."),
                harness,
                model: model.into(),
                model_settings: None,
                files: Vec::new(),
                depends_on: Vec::new(),
                extra: Default::default(),
            };
            let proposal = OrchestrationProposal {
                version: 1,
                lead_id: "gallery".into(),
                cwd: "/Users/dev/widgets".into(),
                checkout_cwd: None,
                request: "Split the card port".into(),
                author: choice(HarnessId::Claude, "claude:opus-4.6", "Opus 4.6"),
                settings: OrchestrationSettings {
                    choices: vec![
                        choice(HarnessId::Claude, "claude:sonnet-4.5", "Sonnet 4.5"),
                        choice(HarnessId::Codex, "codex:gpt-5-codex", "GPT-5 Codex"),
                    ],
                    max_workers: 2,
                    extra: Default::default(),
                },
                status: OrchestrationProposalStatus::Ready,
                title: "Port the transcript cards".into(),
                summary: "Three workers split the cards.".into(),
                tasks: vec![
                    task(
                        "t1",
                        "Port the question form",
                        HarnessId::Claude,
                        "claude:sonnet-4.5",
                    ),
                    task(
                        "t2",
                        "Port the link preview",
                        HarnessId::Codex,
                        "codex:gpt-5-codex",
                    ),
                    task(
                        "t3",
                        "Port the find bar",
                        HarnessId::Claude,
                        "claude:sonnet-4.5",
                    ),
                    task(
                        "t4",
                        "Write the gallery",
                        HarnessId::Codex,
                        "codex:gpt-5-codex",
                    ),
                ],
                error: None,
                response: None,
                extra: Default::default(),
            };
            let mut ask = Block::new("u1", BlockRole::User, "Split the card port across workers");
            ask.intent = Some(monocode_core::block::TurnIntent::Orchestrate);
            ask.started_at = Some(monocode_view_transcript::cards::util::now_ms() - 500);
            let mut card = proposal_block("p1", &proposal);
            card.streaming = Some(false);
            let blocks = vec![
                ask,
                Block::new("a1", BlockRole::Assistant, "Here is how I would split it."),
                card,
            ];
            let mut session = monocode_core::Session::blank(
                "gallery",
                HarnessId::Claude,
                "claude:opus-4.6",
                "/Users/dev/widgets",
            );
            session.blocks = blocks;
            let session = Arc::new(session);
            let transcript = cx.new(|cx| {
                let mut view = monocode_view_transcript::transcript::TranscriptView::new(cx);
                view.set_config(Default::default(), cx);
                view.set_session(session, cx);
                view
            });
            page(
                "TranscriptView orchestration result",
                vec![transcript.into()],
                |_, _| Vec::new(),
                false,
            )
        }
        "transcript" | "transcript-plan" => {
            seed_link_cache(cx);
            let now = monocode_view_transcript::cards::util::now_ms();
            let plan_scene = args.card == "transcript-plan";
            let mut blocks: Vec<Block> = Vec::new();
            if plan_scene {
                let mut ask = Block::new(
                    "u1",
                    BlockRole::User,
                    "Plan how to measure speech bubbles with the real font",
                );
                ask.intent = Some(monocode_core::block::TurnIntent::Plan);
                ask.started_at = Some(now - 600);
                blocks.push(ask);
                let mut plan = Block::new(
                    "p1",
                    BlockRole::Plan,
                    "# Measure bubbles with the real font\n\nThe bubble guesses glyph widths, so long words wrap early. Measure with the text system and cap at the pane width.",
                );
                plan.plan = Some(monocode_core::block::PlanBlockMeta {
                    key: None,
                    status: PlanStatus::Ready,
                    original_text: None,
                    approved_text: None,
                    edited: None,
                    extra: Default::default(),
                });
                blocks.push(plan);
                let mut tasks = Block::new("t1", BlockRole::Tasks, "");
                tasks.task_list = Some(monocode_core::block::TaskListMeta {
                    items: vec![
                        TaskListItem {
                            id: None,
                            text: "Add measureText".into(),
                            status: TaskListItemStatus::Completed,
                            extra: Default::default(),
                        },
                        TaskListItem {
                            id: None,
                            text: "Use it in bubbleWidth".into(),
                            status: TaskListItemStatus::InProgress,
                            extra: Default::default(),
                        },
                    ],
                    explanation: None,
                    key: None,
                    provider_session_id: None,
                    extra: Default::default(),
                });
                blocks.push(tasks);
            } else {
                let mut note_turn =
                    Block::new("u1", BlockRole::User, "Turn this note into a checklist");
                note_turn.note_card = Some(NoteCardMeta {
                    id: "n1".into(),
                    slug: "release-plan".into(),
                    title: "Release plan for the native app".into(),
                    source_cwd: None,
                    extra: Default::default(),
                });
                blocks.push(note_turn);
                blocks.push(Block::new(
                    "a1",
                    BlockRole::Assistant,
                    "Here is the checklist.",
                ));
                blocks.push(Block::new(
                    "u2",
                    BlockRole::User,
                    "https://github.com/acme/widgets/pull/73 can you review this?",
                ));
                blocks.push(Block::new(
                    "a2",
                    BlockRole::Assistant,
                    "Looks good. I left two comments.",
                ));
                let mut operator = Block::new("u3", BlockRole::User, "tidy the inbox filters");
                operator.monocode = Some(true);
                operator.started_at = Some(now - 600);
                blocks.push(operator);
                let path =
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/monocode.png");
                let mut image = Block::new("i1", BlockRole::Image, "");
                image.image = Some(GeneratedImageMeta {
                    path: path.to_string_lossy().into_owned(),
                    name: "monocode.png".into(),
                    mime_type: "image/png".into(),
                    size: 0,
                    alt: None,
                    extra: Default::default(),
                });
                blocks.push(image);
            }
            let mut session = monocode_core::Session::blank(
                "gallery",
                HarnessId::Claude,
                "claude:opus-4.6",
                "/Users/dev/widgets",
            );
            session.blocks = blocks;
            let session = Arc::new(session);
            let transcript = cx.new(|cx| {
                let mut view = monocode_view_transcript::transcript::TranscriptView::new(cx);
                view.set_config(
                    monocode_view_transcript::transcript::TranscriptConfig {
                        can_open_plans: true,
                        can_build_plans: true,
                        can_save_notes: true,
                        can_add_to_chat: true,
                        ..Default::default()
                    },
                    cx,
                );
                view.set_session(session, cx);
                view.scroll_to_top(cx);
                view
            });
            page(
                "TranscriptView with cards",
                vec![transcript.into()],
                |_, _| Vec::new(),
                false,
            )
        }
        "spinner" => page(
            "TerminalSpinner",
            Vec::new(),
            |_, cx| {
                let theme = Theme::of(cx).clone();
                vec![
                    div()
                        .flex()
                        .items_center()
                        .gap(u(6.))
                        .text_px(12.)
                        .text_color(theme.content(0.55))
                        .child(terminal_spinner("spinner"))
                        .child("Preparing a handoff")
                        .into_any_element(),
                ]
            },
            true,
        ),
        "bursts" => {
            let elapsed = Duration::from_millis(args.burst_ms);
            page(
                "MonocodeSparkles, PlanStepsBurst",
                Vec::new(),
                move |_, cx| {
                    let theme = Theme::of(cx).clone();
                    let bubble = |id: &str, text: &str, kind: BurstKind, theme: &Theme| {
                        div()
                            .flex()
                            .justify_end()
                            .child(
                                div()
                                    .relative()
                                    .max_w(u(576.))
                                    .rounded(u(12.))
                                    .bg(theme.content(0.1))
                                    .when(kind == BurstKind::Sparkles, |el| {
                                        el.border_1().border_color(monocode_ui::color::with_alpha(
                                            monocode_view_transcript::cards::style::amber_300_80(),
                                            0.4,
                                        ))
                                    })
                                    .px(u(12.))
                                    .py(u(8.))
                                    .text_px(14.)
                                    .child(text.to_string())
                                    .child(
                                        celebration_burst(kind, id.to_string(), Some(0))
                                            .radius(12.)
                                            .frozen_at(elapsed),
                                    ),
                            )
                            .into_any_element()
                    };
                    vec![
                        caption(&format!("{}ms into the burst", elapsed.as_millis()), &theme),
                        bubble(
                            "operator",
                            "/operator tidy the inbox filters and open the failing PR",
                            BurstKind::Sparkles,
                            &theme,
                        ),
                        div().h(u(40.)).into_any_element(),
                        bubble(
                            "plan",
                            "Plan how to measure speech bubbles with the real font metrics",
                            BurstKind::PlanSteps,
                            &theme,
                        ),
                    ]
                },
                true,
            )
        }
        other => {
            eprintln!("unknown card {other}\n{USAGE}");
            std::process::exit(2);
        }
    }
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    gpui_platform::application()
        .with_assets(monocode_ui::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            let appearance = AppearanceSettings {
                theme_preference: if args.light {
                    ThemePreference::Light
                } else {
                    ThemePreference::Dark
                },
                ..Default::default()
            };
            monocode_ui::init(appearance, cx);
            monocode_view_transcript::transcript::init(cx);
            let (width, height) = args.size.unwrap_or_else(|| default_size(&args.card));
            let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
            let screenshot = args.screenshot.is_some();
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // A screenshot window stays hidden and unfocused.
                focus: !screenshot,
                show: !screenshot,
                ..Default::default()
            };
            let args_for_window = args.clone();
            let window = cx
                .open_window(options, move |window, cx| {
                    monocode_ui::sync_window(window, cx);
                    window.set_background_appearance(gpui::WindowBackgroundAppearance::Opaque);
                    let gallery = build(&args_for_window, window, cx);
                    let gallery: Entity<Gallery> = cx.new(|_| gallery);
                    cx.new(|cx| Root::new(gallery, window, cx))
                })
                .expect("open the gallery window");
            match args.screenshot.clone() {
                Some(out) => capture_and_quit(window.into(), out, cx),
                None => cx.activate(true),
            }
        });
}

/// Redraw until images and fonts settle, then write the frame as a PNG.
fn capture_and_quit(window: gpui::AnyWindowHandle, out: PathBuf, cx: &mut App) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        for _ in 0..15 {
            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;
            let _ = window.update(cx, |_, window, cx| {
                window.dispatch_event(
                    gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                        position: point(px(-1000.), px(-1000.)),
                        ..Default::default()
                    }),
                    cx,
                );
                window.refresh();
                window.draw(cx).clear();
            });
        }
        let result = window.update(cx, |_, window, cx| {
            window.draw(cx).clear();
            window.render_to_image()
        });
        let code = match result {
            Ok(Ok(image)) => {
                if let Some(parent) = out.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match image.save(&out) {
                    Ok(()) => {
                        eprintln!(
                            "wrote {} ({}x{})",
                            out.display(),
                            image.width(),
                            image.height()
                        );
                        0
                    }
                    Err(err) => {
                        eprintln!("screenshot failed: {err}");
                        1
                    }
                }
            }
            Ok(Err(err)) => {
                eprintln!("screenshot failed: {err:#}");
                1
            }
            Err(err) => {
                eprintln!("screenshot failed: {err:#}");
                1
            }
        };
        cx.update(|cx| cx.quit());
        std::process::exit(code);
    })
    .detach();
}

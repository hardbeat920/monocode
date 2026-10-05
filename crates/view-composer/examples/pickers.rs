//! Renders the composer pickers to PNGs, offscreen.
//!
//! ```sh
//! cargo run -p monocode-view-composer --example pickers -- target/pickers-gallery
//! ```
//!
//! Each scene opens a headless window with the platform text system and the
//! Metal headless renderer (macOS), opens its popovers, waits for the open
//! animation, and writes `<scene>.png` at 2x scale through
//! `Window::render_to_image`.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyView, App, AppContext as _, Context, HeadlessAppContext, IntoElement, ParentElement as _,
    Render, Styled as _, Window, WindowHandle, div, px, size,
};
use monocode_core::models::{
    AgentModel, ModelCatalog, ModelPrefs, ModelProvider, ModelSetting, ModelSettingChoice,
    ModelSettingKind,
};
use monocode_core::{HarnessId, ModelSettings, ProjectProviders, RuntimeMode};
use monocode_ui::{AppearanceSettings, Theme, ThemePreference};
use monocode_view_composer::pickers::file_mention_picker::MentionFile;
use monocode_view_composer::pickers::model_source::all_available;
use monocode_view_composer::pickers::{
    AccessPicker, CreateSkillForm, LocalModelSource, McpAvailability, McpServerPicker,
    McpServerRow, ModelControlPills, ModelPicker, ModelPickerProps, ModelSettingsView, ModelSource,
    PickerSkill, SearchableSelect, SearchableSelectOption, SelectVariant, SessionFolderPicker,
    SessionFolderRow, SkillCompletions, SkillDocumentPreview, SkillPromptField, SkillScope,
    SkillTextPart, SlashToken, file_mention_picker, shimmer, skill_picker,
};

/// A window body: pickers along the bottom, like the composer toolbar.
struct Stage {
    rows: Vec<Vec<AnyView>>,
    bottom: bool,
}

impl Render for Stage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(16.))
            .p(px(24.))
            .bg(theme.colors.background_base)
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone())
            .line_height(gpui::relative(theme.leading.normal));
        if self.bottom {
            root = root.justify_end();
        }
        for row in &self.rows {
            root = root.child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(4.))
                    .children(row.iter().cloned()),
            );
        }
        root
    }
}

fn select(id: &str, label: &str, value: &str, options: &[(&str, &str)]) -> ModelSetting {
    ModelSetting {
        id: id.into(),
        label: label.into(),
        kind: ModelSettingKind::Select,
        value: value.into(),
        options: options
            .iter()
            .map(|(value, label)| ModelSettingChoice {
                value: (*value).into(),
                label: (*label).into(),
            })
            .collect(),
        description: None,
    }
}

fn toggle(id: &str, label: &str) -> ModelSetting {
    ModelSetting {
        kind: ModelSettingKind::Toggle,
        ..select(id, label, "false", &[("false", "Off"), ("true", "On")])
    }
}

fn catalog() -> ModelCatalog {
    let mut catalog = ModelCatalog::new();
    let mut opus = AgentModel::new("claude:opus-5", HarnessId::Claude, "Claude Opus 5")
        .with_native_id("claude-opus-5");
    opus.settings = Some(vec![
        select(
            "effort",
            "Reasoning",
            "high",
            &[
                ("medium", "Medium"),
                ("high", "High"),
                ("xhigh", "Extra High"),
                ("max", "Max"),
            ],
        ),
        toggle("fast", "Fast"),
    ]);
    let sonnet = AgentModel::new("claude:sonnet-5", HarnessId::Claude, "Claude Sonnet 5")
        .with_native_id("claude-sonnet-5");
    let opus55 = AgentModel::new("claude:opus-5-5", HarnessId::Claude, "Claude Opus 5.5")
        .with_native_id("claude-opus-5-5");
    let haiku = AgentModel::new("claude:haiku-4.5", HarnessId::Claude, "Haiku 4.5")
        .with_native_id("claude-haiku-4-5");
    catalog.set_harness_models(HarnessId::Claude, vec![sonnet, opus, opus55, haiku]);
    let provider = |id: &str, name: &str| {
        Some(ModelProvider {
            id: id.into(),
            name: name.into(),
        })
    };
    let mut go = AgentModel::new("opencode:opencode-go/glm-5", HarnessId::Opencode, "GLM 5");
    go.provider = provider("opencode-go", "OpenCode Go");
    let mut kimi = AgentModel::new(
        "opencode:opencode-go/kimi-k2.5",
        HarnessId::Opencode,
        "Kimi K2.5",
    );
    kimi.provider = provider("opencode-go", "OpenCode Go");
    let mut luna = AgentModel::new(
        "opencode:openai/gpt-5.6-luna",
        HarnessId::Opencode,
        "GPT-5.6 Luna",
    );
    luna.provider = provider("openai", "OpenAI");
    catalog.set_harness_models(HarnessId::Opencode, vec![go, kimi, luna]);
    catalog
}

fn source() -> Rc<dyn ModelSource> {
    let mut availability = all_available();
    availability.installed.remove(&HarnessId::Hermes);
    Rc::new(LocalModelSource::new(catalog(), availability))
}

fn prefs() -> ModelPrefs {
    let mut prefs = ModelPrefs {
        favorite_models: vec![
            "claude:opus-5".into(),
            "opencode:openai/gpt-5.6-luna".into(),
        ],
        ..Default::default()
    };
    prefs.save_recent_model_choice(HarnessId::Grok, "grok:grok-4.6");
    prefs.save_recent_model_choice(HarnessId::Cursor, "cursor:composer-2.5");
    prefs.save_recent_model_choice(HarnessId::Opencode, "opencode:openai/gpt-5.6-luna");
    prefs
}

fn values(pairs: &[(&str, &str)]) -> ModelSettings {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn opus_props(hide_settings: bool) -> ModelPickerProps {
    ModelPickerProps {
        harness: Some(HarnessId::Claude),
        model: "claude:opus-5".into(),
        values: values(&[("effort", "high"), ("fast", "false")]),
        hide_settings,
        hotkeys: true,
        ..Default::default()
    }
}

fn model_picker(
    props: ModelPickerProps,
    window: &mut Window,
    cx: &mut App,
) -> gpui::Entity<ModelPicker> {
    cx.new(|cx| {
        ModelPicker::new(
            props,
            source(),
            prefs(),
            ProjectProviders::default(),
            window,
            cx,
        )
    })
}

/// The engine's slash rules, simplified for the gallery.
struct GalleryCompletions(Vec<PickerSkill>);

impl SkillCompletions for GalleryCompletions {
    fn slash_token_at(&self, text: &str, cursor: usize) -> Option<SlashToken> {
        let before = &text[..cursor.min(text.len())];
        let start = before
            .rfind(char::is_whitespace)
            .map(|at| at + 1)
            .unwrap_or(0);
        if !text[start..].starts_with('/') {
            return None;
        }
        let end = text[start..]
            .find(char::is_whitespace)
            .map(|at| start + at)
            .unwrap_or(text.len());
        Some(SlashToken {
            start,
            end,
            query: text[start + 1..cursor.max(start + 1)].to_string(),
        })
    }

    fn rank(&self, query: &str) -> Vec<PickerSkill> {
        self.0
            .iter()
            .filter(|skill| skill.invocation.contains(query))
            .cloned()
            .collect()
    }

    fn replace_slash_token(&self, text: &str, token: &SlashToken, name: &str) -> String {
        format!("{}/{name} {}", &text[..token.start], &text[token.end..])
    }

    fn text_parts(&self, text: &str) -> Vec<SkillTextPart> {
        text.split_inclusive(' ')
            .map(|word| SkillTextPart {
                text: word.to_string(),
                skill: word
                    .trim_end()
                    .strip_prefix('/')
                    .is_some_and(|name| self.0.iter().any(|s| s.invocation == name)),
            })
            .collect()
    }
}

fn skills() -> Vec<PickerSkill> {
    vec![
        PickerSkill::builtin(
            "plan",
            "Create a reviewable implementation plan before changing files.",
        ),
        PickerSkill::builtin(
            "compact",
            "Summarize older conversation context to free space.",
        ),
        PickerSkill::builtin(
            "add-to-folder",
            "Place this session in an existing or new sidebar folder.",
        ),
        PickerSkill::file(
            "deploy",
            "Prepare a deployment and run the release checklist.",
            SkillScope::Project,
            "agents",
        ),
        PickerSkill::file(
            "review",
            "Review the current changes for bugs and style.",
            SkillScope::User,
            "agents",
        ),
    ]
}

fn mention(relative: &str, positions: Vec<usize>, is_dir: bool) -> MentionFile {
    let name = relative.rsplit('/').next().unwrap_or(relative).to_string();
    MentionFile {
        path: format!("/repo/{relative}").into(),
        relative: relative.to_string().into(),
        name: name.into(),
        is_dir,
        positions,
    }
}

fn mcp_rows(query: &str) -> Vec<McpServerRow> {
    let row =
        |name: &str, icon, label: &str, scope: &str, availability, detail: &str| McpServerRow {
            key: format!("{label}:{scope}:{name}").into(),
            name: name.to_string().into(),
            icon,
            provider_label: label.to_string().into(),
            scope: scope.to_string().into(),
            availability,
            detail: detail.to_string().into(),
        };
    vec![
        row(
            "context7",
            HarnessId::Claude,
            "Claude Code",
            "user",
            McpAvailability::Available,
            "",
        ),
        row(
            "github",
            HarnessId::Claude,
            "Claude Code",
            "project",
            McpAvailability::Available,
            "",
        ),
        row(
            "linear",
            HarnessId::Claude,
            "Claude Code",
            "user",
            McpAvailability::Authentication,
            "",
        ),
        row(
            "figma",
            HarnessId::Cursor,
            "Cursor",
            "user",
            McpAvailability::Unavailable,
            "Different provider",
        ),
    ]
    .into_iter()
    .filter(|row| row.name.contains(query))
    .collect()
}

type Build = Box<dyn FnOnce(&mut Window, &mut App) -> Stage>;

struct Scene {
    name: &'static str,
    width: f32,
    height: f32,
    light: bool,
    build: Build,
}

fn scene(
    name: &'static str,
    width: f32,
    height: f32,
    build: impl FnOnce(&mut Window, &mut App) -> Stage + 'static,
) -> Scene {
    Scene {
        name,
        width,
        height,
        light: false,
        build: Box::new(build),
    }
}

fn stage(rows: Vec<Vec<AnyView>>) -> Stage {
    Stage { rows, bottom: true }
}

fn scenes() -> Vec<Scene> {
    let mut list = vec![
        scene("model-menu", 760., 600., |window, cx| {
            let picker = model_picker(opus_props(false), window, cx);
            picker.update(cx, |picker, cx| {
                picker.toggle_picker(window, cx);
                // Highlight the Model row (Up wraps to it) and open its flyout.
                picker.menu_key("up", window, cx);
                picker.menu_key("right", window, cx);
            });
            stage(vec![vec![picker.into()]])
        }),
        scene("model-effort-submenu", 560., 360., |window, cx| {
            let picker = model_picker(opus_props(false), window, cx);
            picker.update(cx, |picker, cx| {
                picker.toggle_picker(window, cx);
                picker.menu_key("down", window, cx);
                picker.menu_key("right", window, cx);
            });
            stage(vec![vec![picker.into()]])
        }),
        scene("model-flyout-opencode", 520., 600., |window, cx| {
            let mut props = opus_props(true);
            props.harness = Some(HarnessId::Opencode);
            props.model = "opencode:openai/gpt-5.6-luna".into();
            let picker = model_picker(props, window, cx);
            picker.update(cx, |picker, cx| picker.toggle_picker(window, cx));
            stage(vec![vec![picker.into()]])
        }),
        scene("model-favorites", 520., 600., |window, cx| {
            let picker = model_picker(opus_props(true), window, cx);
            picker.update(cx, |picker, cx| {
                picker.toggle_picker(window, cx);
                picker.select_tab(monocode_core::models::ModelPickerTab::Favorites, window, cx);
            });
            stage(vec![vec![picker.into()]])
        }),
        scene("model-recent", 420., 320., |window, cx| {
            let mut props = opus_props(false);
            props.harness = Some(HarnessId::Grok);
            props.model = "grok:grok-4.6".into();
            props.values = values(&[("effort", "high")]);
            let picker = model_picker(props, window, cx);
            picker.update(cx, |picker, cx| picker.open_recent_menu(window, cx));
            stage(vec![vec![picker.into()]])
        }),
        scene("model-pills", 560., 360., |window, cx| {
            let picker = model_picker(opus_props(true), window, cx);
            let pills = cx.new(|cx| {
                ModelControlPills::new(
                    HarnessId::Claude,
                    "claude:opus-5",
                    values(&[("effort", "high"), ("fast", "true")]),
                    source(),
                    cx,
                )
            });
            pills.update(cx, |pills, cx| {
                let pill = pills.pills().into_iter().next();
                if let Some(monocode_view_composer::pickers::model_logic::ControlPill::Select {
                    setting,
                    grouped,
                }) = pill
                {
                    pills.toggle_select(&setting, &grouped, window, cx);
                }
            });
            let access = cx.new(|cx| AccessPicker::new(RuntimeMode::FullAccess, cx));
            stage(vec![vec![picker.into(), pills.into(), access.into()]])
        }),
        scene("access-picker", 420., 420., |window, cx| {
            let access = cx.new(|cx| AccessPicker::new(RuntimeMode::AutoAcceptEdits, cx));
            access.update(cx, |access, cx| {
                access.set_busy(true, cx);
                access.toggle(window, cx);
            });
            stage(vec![vec![access.into()]])
        }),
        scene("model-settings", 520., 320., |window, cx| {
            let view = cx.new(|cx| {
                ModelSettingsView::new(
                    HarnessId::Claude,
                    "claude:opus-5",
                    values(&[("effort", "xhigh"), ("fast", "true")]),
                    source(),
                    cx,
                )
            });
            view.update(cx, |view, cx| {
                let first = view
                    .settings()
                    .into_iter()
                    .find(|s| s.kind == ModelSettingKind::Select);
                if let Some(setting) = first {
                    view.toggle_select(&setting, window, cx);
                }
            });
            stage(vec![vec![view.into()]])
        }),
        scene("searchable-select", 560., 420., |window, cx| {
            let options = [
                "Codex",
                "Claude Code",
                "Cursor",
                "Grok Build",
                "OpenCode",
                "Pi",
                "Factory Droid",
            ]
            .into_iter()
            .map(|name| SearchableSelectOption::new(name.to_lowercase(), name))
            .collect::<Vec<_>>();
            let field = cx.new(|cx| {
                SearchableSelect::new("Agent", "claude code", options.clone(), window, cx)
            });
            let pill = cx.new(|cx| {
                SearchableSelect::new(
                    "Timeout",
                    "60",
                    vec![
                        SearchableSelectOption::new("30", "30 sec"),
                        SearchableSelectOption::new("60", "1 min"),
                        SearchableSelectOption::new("120", "2 min"),
                    ],
                    window,
                    cx,
                )
                .variant(SelectVariant::Pill)
                .searchable(false)
            });
            field.update(cx, |field, cx| field.open_menu(window, cx));
            let mut stage = stage(vec![
                vec![pill.into()],
                vec![div_view(cx, 320., field.into())],
            ]);
            stage.bottom = false;
            stage
        }),
        scene("mcp-picker", 520., 340., |window, cx| {
            let picker = cx.new(|cx| McpServerPicker::new(mcp_rows, window, cx));
            stage(vec![vec![div_view(cx, 460., picker.into())]])
        }),
        scene("session-folder-picker", 520., 300., |window, cx| {
            let folders = vec![
                SessionFolderRow {
                    id: "work".into(),
                    name: "Work".into(),
                    session_count: 12,
                },
                SessionFolderRow {
                    id: "launch".into(),
                    name: "Launch prep".into(),
                    session_count: 3,
                },
                SessionFolderRow {
                    id: "bugs".into(),
                    name: "Bug bash".into(),
                    session_count: 7,
                },
            ];
            let picker = cx.new(|cx| SessionFolderPicker::new(folders, window, cx));
            stage(vec![vec![div_view(cx, 460., picker.into())]])
        }),
        scene("file-mentions", 520., 300., |_, cx| {
            let files = vec![
                mention("src/main.rs", vec![4, 5, 6], false),
                mention(
                    "crates/view-composer/src/pickers/model_picker.rs",
                    vec![32, 33],
                    false,
                ),
                mention("crates/ui", vec![0], true),
                MentionFile {
                    path: "note:abc".into(),
                    relative: "note/auth".into(),
                    name: "note/auth".into(),
                    is_dir: false,
                    positions: vec![],
                },
            ];
            let view = cx.new(|_| {
                Static(Box::new(move |_, _| {
                    file_mention_picker("mentions", files.clone(), "ma", 1).into_any_element()
                }))
            });
            stage(vec![vec![div_view(cx, 460., view.into())]])
        }),
        scene("skill-picker", 520., 380., |_, cx| {
            let view = cx.new(|_| {
                Static(Box::new(move |_, _| {
                    skill_picker("skills", skills(), "", 3).into_any_element()
                }))
            });
            stage(vec![vec![div_view(cx, 460., view.into())]])
        }),
        scene("skill-create", 520., 300., |window, cx| {
            let form = cx.new(|cx| CreateSkillForm::new("Release Notes!", true, window, cx));
            let view = cx.new(|_| {
                Static(Box::new(move |_, _| {
                    skill_picker("skills", skills(), "", 0)
                        .creating(Some(form.clone()))
                        .into_any_element()
                }))
            });
            stage(vec![vec![div_view(cx, 460., view.into())]])
        }),
        scene("skill-prompt-field", 560., 420., |window, cx| {
            let completions: Rc<dyn SkillCompletions> = Rc::new(GalleryCompletions(skills()));
            let field = cx.new(|cx| {
                SkillPromptField::new(
                    "Every morning, run /review on open pull requests, then /",
                    completions,
                    window,
                    cx,
                )
            });
            field.update(cx, |field, cx| {
                let input = field.input().clone();
                input.update(cx, |input, cx| {
                    input.focus(window, cx);
                    let end = input.value().len();
                    input.set_selected_range(end..end, cx);
                });
            });
            let mut stage = stage(vec![vec![div_view(cx, 500., field.into())]]);
            stage.bottom = false;
            stage
        }),
        scene("skill-document", 620., 420., |_, cx| {
            let preview = cx.new(|cx| {
                let mut preview = SkillDocumentPreview::new(
                    "---\nname: deploy\ndescription: Prepare a deployment.\n---\n\n# Deploy\n\nRun the release checklist, then tag the build.\n\n- Bump the version\n- Update the changelog\n",
                    cx,
                );
                preview.toggle_metadata(cx);
                preview
            });
            let mut stage = stage(vec![vec![div_view(cx, 560., preview.into())]]);
            stage.bottom = false;
            stage
        }),
        scene("shimmer", 360., 120., |_, cx| {
            let view = cx.new(|_| {
                Static(Box::new(|_, _| {
                    div()
                        .text_size(px(14.))
                        .child(shimmer("thinking", "Thinking about the plan…").duration(1.6))
                        .into_any_element()
                }))
            });
            stage(vec![vec![view.into()]])
        }),
    ];
    let light_menu = scene("model-menu-light", 760., 600., |window, cx| {
        let picker = model_picker(opus_props(false), window, cx);
        picker.update(cx, |picker, cx| {
            picker.toggle_picker(window, cx);
            picker.menu_key("up", window, cx);
            picker.menu_key("right", window, cx);
        });
        stage(vec![vec![picker.into()]])
    });
    list.push(Scene {
        light: true,
        ..light_menu
    });
    let light_mcp = scene("mcp-picker-light", 520., 340., |window, cx| {
        let picker = cx.new(|cx| McpServerPicker::new(mcp_rows, window, cx));
        stage(vec![vec![div_view(cx, 460., picker.into())]])
    });
    list.push(Scene {
        light: true,
        ..light_mcp
    });
    list
}

/// A view that renders a builder each frame, for the controlled elements.
type Builder = Box<dyn Fn(&mut Window, &mut App) -> gpui::AnyElement>;

struct Static(Builder);

impl Render for Static {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        (self.0)(window, cx)
    }
}

/// Wraps a view in a fixed-width column.
fn div_view(cx: &mut App, width: f32, view: AnyView) -> AnyView {
    cx.new(|_| {
        Static(Box::new(move |_, _| {
            div().w(px(width)).child(view.clone()).into_any_element()
        }))
    })
    .into()
}

fn capture(cx: &mut HeadlessAppContext, scene: Scene, out: &Path) {
    let appearance = AppearanceSettings {
        theme_preference: if scene.light {
            ThemePreference::Light
        } else {
            ThemePreference::Dark
        },
        ..Default::default()
    };
    cx.update(|cx| monocode_ui::set_appearance(appearance, cx));
    let build = scene.build;
    let window: WindowHandle<Stage> = cx
        .open_window(
            size(px(scene.width), px(scene.height)),
            move |window, cx| {
                monocode_ui::sync_window(window, cx);
                let stage = build(window, cx);
                cx.new(|_| stage)
            },
        )
        .expect("open window");
    let draw = |cx: &mut HeadlessAppContext| {
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
        })
        .expect("draw");
        cx.run_until_parked();
    };
    // Let assets load and the popover open animation finish in real time.
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(450) {
        draw(cx);
        std::thread::sleep(Duration::from_millis(16));
    }
    draw(cx);
    let image = cx.capture_screenshot(window.into()).expect("capture");
    let path = out.join(format!("{}.png", scene.name));
    image.save(&path).expect("save png");
    eprintln!(
        "wrote {} ({}x{})",
        path.display(),
        image.width(),
        image.height()
    );
    cx.update_window(window.into(), |_, window, _| window.remove_window())
        .ok();
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/pickers-gallery"));
    let only = std::env::args().nth(2);
    std::fs::create_dir_all(&out).expect("create output dir");
    let platform = gpui_platform::current_platform(true);
    let mut cx = HeadlessAppContext::with_platform(
        platform.text_system(),
        Arc::new(monocode_ui::Assets),
        gpui_platform::current_headless_renderer,
    );
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(AppearanceSettings::default(), cx);
        monocode_view_composer::pickers::init(cx);
    });
    for scene in scenes() {
        if only.as_deref().is_some_and(|only| only != scene.name) {
            continue;
        }
        capture(&mut cx, scene, &out);
    }
}

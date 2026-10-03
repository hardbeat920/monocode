//! Ports of SessionFolderPicker.test.ts, SkillPromptField.test.ts, and
//! SearchableSelect.test.ts, plus wiring checks for the access, MCP, and
//! skill pickers.

use std::rc::Rc;

use gpui::{
    AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement as _, Render,
    Styled as _, TestAppContext, VisualTestContext, Window, div, px,
};
use monocode_core::{HarnessId, RuntimeMode};

use super::{Calls, Host, bounds, click, draw, exists, init, keys};
use crate::pickers::file_mention_picker::MentionFile;
use crate::pickers::skill_picker::skill_row_texts;
use crate::pickers::{
    AccessPicker, DismissReason, McpAvailability, McpServerPicker, McpServerRow, PickerSkill,
    SearchableSelect, SearchableSelectOption, SelectVariant, SessionFolderPicker, SessionFolderRow,
    SessionFolderTarget, SkillCompletions, SkillDocumentPreview, SkillPromptField, SkillScope,
    SkillTextPart, SlashToken, file_mention_picker, skill_picker,
};

fn mount<V: Render + 'static>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut Context<Host>) -> Entity<V>,
) -> (Entity<V>, &mut VisualTestContext) {
    init(cx);
    let (host, cx) = cx.add_window_view(|window, cx| {
        let view = build(window, cx);
        Host {
            children: vec![view.into()],
        }
    });
    let view = host.read_with(cx, |host, _| host.children[0].clone());
    draw(cx);
    (view.downcast::<V>().unwrap(), cx)
}

// session folder picker

fn work() -> SessionFolderRow {
    SessionFolderRow {
        id: "work".into(),
        name: "Work".into(),
        session_count: 2,
    }
}

#[gpui::test]
fn chooses_an_existing_sidebar_folder(cx: &mut TestAppContext) {
    let picks = Calls::new();
    let record = picks.recorder();
    let (_, cx) = mount(cx, |window, cx| {
        cx.new(|cx| {
            SessionFolderPicker::new(vec![work()], window, cx)
                .on_pick(move |target, _, _| record(target.clone()))
        })
    });
    assert!(exists(cx, "session-folder-Work"));
    click(cx, "session-folder-Work");
    assert_eq!(
        picks.all(),
        vec![SessionFolderTarget::Existing {
            folder_id: "work".into()
        }]
    );
}

#[gpui::test]
fn creates_a_named_sidebar_folder(cx: &mut TestAppContext) {
    let picks = Calls::new();
    let record = picks.recorder();
    let (_, cx) = mount(cx, |window, cx| {
        cx.new(|cx| {
            SessionFolderPicker::new(Vec::new(), window, cx)
                .on_pick(move |target, _, _| record(target.clone()))
        })
    });
    cx.simulate_input("Launch work");
    draw(cx);
    assert!(exists(cx, "session-folder-Create “Launch work”"));
    click(cx, "session-folder-Create “Launch work”");
    assert_eq!(
        picks.all(),
        vec![SessionFolderTarget::New {
            name: "Launch work".into()
        }]
    );
}

#[gpui::test]
fn folder_keys_wrap_and_enter_picks(cx: &mut TestAppContext) {
    let picks = Calls::new();
    let record = picks.recorder();
    let other = SessionFolderRow {
        id: "home".into(),
        name: "Home".into(),
        session_count: 0,
    };
    let (picker, cx) = mount(cx, |window, cx| {
        cx.new(|cx| {
            SessionFolderPicker::new(vec![work(), other], window, cx)
                .on_pick(move |target, _, _| record(target.clone()))
        })
    });
    keys(cx, "up");
    assert_eq!(picker.read_with(cx, |p, _| p.active()), 1);
    keys(cx, "down enter");
    assert_eq!(
        picks.all(),
        vec![SessionFolderTarget::Existing {
            folder_id: "work".into()
        }]
    );
}

// SkillPromptField

/// Stands in for the engine's slash rules: a `/` word under the cursor,
/// ranked by prefix.
struct FakeCompletions {
    skills: Vec<PickerSkill>,
}

impl SkillCompletions for FakeCompletions {
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
        if cursor > end {
            return None;
        }
        Some(SlashToken {
            start,
            end,
            query: text[start + 1..cursor].to_string(),
        })
    }

    fn rank(&self, query: &str) -> Vec<PickerSkill> {
        self.skills
            .iter()
            .filter(|skill| skill.invocation.starts_with(query))
            .cloned()
            .collect()
    }

    fn replace_slash_token(&self, text: &str, token: &SlashToken, name: &str) -> String {
        let rest = &text[token.end..];
        let spacer = if rest.starts_with(' ') { "" } else { " " };
        format!("{}/{name}{spacer}{rest}", &text[..token.start])
    }

    fn text_parts(&self, text: &str) -> Vec<SkillTextPart> {
        let mut parts: Vec<SkillTextPart> = Vec::new();
        for word in text.split_inclusive(' ') {
            let token = word.trim_end();
            let skill = token
                .strip_prefix('/')
                .is_some_and(|name| self.skills.iter().any(|s| s.invocation == name));
            if skill {
                parts.push(SkillTextPart {
                    text: token.to_string(),
                    skill: true,
                });
                if token.len() < word.len() {
                    parts.push(SkillTextPart {
                        text: word[token.len()..].to_string(),
                        skill: false,
                    });
                }
            } else {
                parts.push(SkillTextPart {
                    text: word.to_string(),
                    skill: false,
                });
            }
        }
        parts
    }
}

fn completions() -> Rc<dyn SkillCompletions> {
    Rc::new(FakeCompletions {
        skills: vec![
            PickerSkill::file(
                "deploy",
                "Prepare a deployment",
                SkillScope::Project,
                "agents",
            ),
            PickerSkill::file(
                "review",
                "Review the current changes",
                SkillScope::Project,
                "agents",
            ),
        ],
    })
}

fn mount_field(
    cx: &mut TestAppContext,
) -> (
    Entity<SkillPromptField>,
    Calls<String>,
    &mut VisualTestContext,
) {
    let changes = Calls::new();
    let record = changes.recorder();
    let (field, cx) = mount(cx, |window, cx| {
        cx.new(|cx| {
            SkillPromptField::new("", completions(), window, cx)
                .on_change(move |value, _, _| record(value.to_string()))
        })
    });
    cx.update(|window, cx| {
        let input = field.read(cx).input().clone();
        input.update(cx, |input, cx| input.focus(window, cx));
    });
    draw(cx);
    (field, changes, cx)
}

fn skill_parts(field: &Entity<SkillPromptField>, cx: &mut VisualTestContext) -> Vec<String> {
    field.read_with(cx, |field, _| {
        completions()
            .text_parts(field.value())
            .into_iter()
            .filter(|part| part.skill)
            .map(|part| part.text)
            .collect()
    })
}

#[gpui::test]
fn opens_the_shared_picker_for_slash_input_and_inserts_a_keyboard_selection(
    cx: &mut TestAppContext,
) {
    let (field, changes, cx) = mount_field(cx);
    cx.simulate_input("/rev");
    draw(cx);
    assert!(exists(cx, "skill-prompt-popover"));
    assert_eq!(bounds(cx, "skill-prompt-popover").size.width, px(298.));
    assert!(exists(cx, "skill-row-review"));
    assert!(!exists(cx, "skill-picker-new"));
    let ranked = field.read_with(cx, |field, _| field.ranked().to_vec());
    assert_eq!(
        skill_row_texts(&ranked[0], true),
        vec!["/review", "project"]
    );

    keys(cx, "enter");
    assert_eq!(
        field.read_with(cx, |field, _| field.value().to_string()),
        "/review "
    );
    assert_eq!(changes.last(), Some("/review ".to_string()));
    assert!(!exists(cx, "skill-prompt-popover"));
    assert_eq!(skill_parts(&field, cx), vec!["/review"]);
}

#[gpui::test]
fn inserts_a_clicked_skill_and_returns_focus_to_the_instructions(cx: &mut TestAppContext) {
    let (field, _, cx) = mount_field(cx);
    cx.simulate_input("Run /de");
    draw(cx);
    click(cx, "skill-row-deploy");
    assert_eq!(
        field.read_with(cx, |field, _| field.value().to_string()),
        "Run /deploy "
    );
    let focused = cx.update(|window, cx| {
        let input = field.read(cx).input().clone();
        input.focus_handle(cx).is_focused(window)
    });
    assert!(focused);
    assert_eq!(skill_parts(&field, cx), vec!["/deploy"]);
}

#[gpui::test]
fn the_highlight_mirror_starts_where_the_input_text_starts(cx: &mut TestAppContext) {
    let (field, _, cx) = mount_field(cx);
    cx.simulate_input("Run /deploy now");
    draw(cx);
    let frame = bounds(cx, "skill-prompt-field");
    let first = cx.update(|_, cx| {
        let input = field.read(cx).input().clone();
        input.read(cx).range_to_bounds(&(0..1)).expect("laid out")
    });
    // `px-3 py-3` at the default 16px rem.
    assert_eq!(first.origin.x - frame.origin.x, px(12.));
    assert_eq!(first.origin.y - frame.origin.y, px(12.));
}

#[gpui::test]
fn keeps_the_picker_dismissed_after_escape(cx: &mut TestAppContext) {
    let (field, _, cx) = mount_field(cx);
    cx.simulate_input("/rev");
    draw(cx);
    assert!(exists(cx, "skill-prompt-popover"));
    keys(cx, "escape");
    draw(cx);
    assert!(!exists(cx, "skill-prompt-popover"));
    assert_eq!(
        field.read_with(cx, |field, _| field.value().to_string()),
        "/rev"
    );
}

// SearchableSelect

fn agents() -> Vec<SearchableSelectOption> {
    vec![
        SearchableSelectOption::new("codex", "Codex"),
        SearchableSelectOption::new("claude", "Claude"),
    ]
}

#[gpui::test]
fn searches_and_picks_an_option(cx: &mut TestAppContext) {
    let picks = Calls::new();
    let record = picks.recorder();
    let (select, cx) = mount(cx, |window, cx| {
        cx.new(|cx| {
            SearchableSelect::new("Agent", "codex", agents(), window, cx)
                .on_change(move |value, _, _| record(value.to_string()))
        })
    });
    assert_eq!(
        select.read_with(cx, |s, _| s.trigger_label()),
        "Agent: Codex"
    );
    click(cx, "searchable-select-trigger");
    assert!(exists(cx, "searchable-select-menu"));
    cx.simulate_input("cla");
    draw(cx);
    assert_eq!(select.read_with(cx, |s, _| s.filtered().len()), 1);
    keys(cx, "enter");
    assert_eq!(picks.all(), vec!["claude".to_string()]);
    assert!(!exists(cx, "searchable-select-menu"));
}

#[gpui::test]
fn compact_menus_use_short_rows(cx: &mut TestAppContext) {
    let options = vec![
        SearchableSelectOption::new("30", "30 sec"),
        SearchableSelectOption::new("60", "1 min"),
        SearchableSelectOption::new("120", "2 min"),
        SearchableSelectOption::new("300", "5 min"),
    ];
    let (select, cx) = mount(cx, |window, cx| {
        cx.new(|cx| {
            SearchableSelect::new("Timeout", "60", options, window, cx)
                .variant(SelectVariant::Pill)
                .searchable(false)
        })
    });
    click(cx, "searchable-select-trigger");
    assert_eq!(
        bounds(cx, "searchable-select-option-1 min").size.height,
        px(28.)
    );
    assert_eq!(select.read_with(cx, |s, _| s.active()), 1);
    keys(cx, "end");
    assert_eq!(select.read_with(cx, |s, _| s.active()), 3);
    keys(cx, "escape");
    assert!(!select.read_with(cx, |s, _| s.is_open()));
}

// AccessPicker

#[gpui::test]
fn picks_a_runtime_mode_from_the_keyboard(cx: &mut TestAppContext) {
    let picks = Calls::new();
    let record = picks.recorder();
    let (picker, cx) = mount(cx, |_, cx| {
        cx.new(|cx| {
            AccessPicker::new(RuntimeMode::Supervised, cx).on_change(move |mode, _, _| record(mode))
        })
    });
    click(cx, "access-picker-trigger");
    assert!(exists(cx, "access-option-full-access"));
    keys(cx, "down enter");
    assert_eq!(picks.all(), vec![RuntimeMode::AutoAcceptEdits]);
    assert!(!picker.read_with(cx, |p, _| p.is_open()));
}

// McpServerPicker

fn servers(query: &str) -> Vec<McpServerRow> {
    let row = |name: &str, availability| McpServerRow {
        key: format!("claude:user::{name}").into(),
        name: name.to_string().into(),
        icon: HarnessId::Claude,
        provider_label: "Claude Code".into(),
        scope: "user".into(),
        availability,
        detail: "Different provider".into(),
    };
    vec![
        row("docs", McpAvailability::Available),
        row("github", McpAvailability::Available),
        row("linear", McpAvailability::Unavailable),
    ]
    .into_iter()
    .filter(|server| server.name.contains(query))
    .collect()
}

#[gpui::test]
fn steps_through_usable_mcp_servers(cx: &mut TestAppContext) {
    let picks = Calls::new();
    let dismissals = Calls::new();
    let (record, dismissed) = (picks.recorder(), dismissals.recorder());
    let (picker, cx) = mount(cx, |window, cx| {
        cx.new(|cx| {
            McpServerPicker::new(servers, window, cx)
                .on_pick(move |row, _, _| record(row.name.to_string()))
                .on_dismiss(move |reason, _, _| dismissed(reason))
        })
    });
    assert!(exists(cx, "mcp-row-linear"));
    keys(cx, "down down");
    assert_eq!(picker.read_with(cx, |p, _| p.active()), 0);
    keys(cx, "up enter");
    assert_eq!(picks.all(), vec!["github".to_string()]);
    cx.simulate_input("lin");
    draw(cx);
    assert_eq!(picker.read_with(cx, |p, _| p.rows().len()), 1);
    keys(cx, "escape");
    assert_eq!(dismissals.all(), vec![DismissReason::Escape]);
}

// Controlled lists and the document preview

struct Lists;

impl Render for Lists {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let file = MentionFile {
            path: "/repo/src/main.rs".into(),
            relative: "src/main.rs".into(),
            name: "main.rs".into(),
            is_dir: false,
            positions: vec![4, 5],
        };
        div()
            .w(px(320.))
            .child(file_mention_picker("mentions", vec![file], "ma", 0))
            .child(skill_picker(
                "skills",
                vec![PickerSkill::builtin("plan", "Create a plan")],
                "",
                0,
            ))
    }
}

#[gpui::test]
fn renders_the_controlled_lists(cx: &mut TestAppContext) {
    let (_, cx) = mount(cx, |_, cx| cx.new(|_| Lists));
    assert!(exists(cx, "mention-row-src/main.rs"));
    assert!(exists(cx, "skill-row-plan"));
    assert!(exists(cx, "skill-picker-new"));
}

#[gpui::test]
fn folds_skill_metadata_into_a_disclosure(cx: &mut TestAppContext) {
    let (preview, cx) = mount(cx, |_, cx| {
        cx.new(|cx| SkillDocumentPreview::new("---\nname: deploy\n---\n\n# Deploy\n\nShip it.", cx))
    });
    assert_eq!(
        preview.read_with(cx, |p, _| p.metadata().map(str::to_string)),
        Some("name: deploy".into())
    );
    assert!(!preview.read_with(cx, |p, _| p.is_metadata_open()));
    click(cx, "skill-metadata-summary");
    assert!(preview.read_with(cx, |p, _| p.is_metadata_open()));
}

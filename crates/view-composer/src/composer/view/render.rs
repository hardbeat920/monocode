//! The composer's element tree, from the JSX of `Composer` and
//! `ComposerAction` in Composer.tsx, with the + menu and mode pills from
//! modeCommands.tsx. The pickers (model, effort and speed pills, access,
//! skills, files, MCP servers, session folders) come from
//! `crate::pickers`.

use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, Context, DragMoveEvent, Entity, ExternalPaths,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, canvas, div,
    prelude::FluentBuilder as _, relative,
};
use monocode_core::HarnessId;
use monocode_ui::styled::{UiStyled as _, glass_backdrop};
use monocode_ui::{IconName, ProviderLogo, Theme, icon, provider_logo, u};

use super::super::host::{ComposerHost, FolderTarget, NewSkillScope};
use super::super::model::mcp::{McpAvailability, mcp_picker_servers, mcp_provider_label};
use super::super::model::mode_commands::Mode;
use super::super::model::paths::pretty_cwd;
use super::super::model::skills::{Skill, SkillKind};
use super::super::prompt_input::{Enter, Escape, MoveDown, MoveUp, Paste, Tab};
use super::colors::{mode_menu_check, mode_menu_icon, mode_pill, orchestrator_badge, stop_button};
use super::keys::folder_rows;
use super::{Composer, ComposerEvent, ComposerProps};
use crate::pickers::anchor::{BoundsCell, DismissReason, Side, anchored_popover, popover_surface};
use crate::pickers::{
    AccessPicker, CreateScope, CreateSkillForm, McpServerPicker, McpServerRow, MentionFile,
    ModelControlPills, ModelPicker, ModelPickerProps, PickerSkill, SessionFolderPicker,
    SessionFolderTarget, file_mention_picker, skill_picker,
};

/// The picker entities the composer owns.
pub(crate) struct BottomBar {
    pub model_picker: Option<Entity<ModelPicker>>,
    pub model_pills: Option<Entity<ModelControlPills>>,
    pub access: Entity<AccessPicker>,
    pub folder_picker: Option<Entity<SessionFolderPicker>>,
    pub mcp_picker: Option<Entity<McpServerPicker>>,
    pub create_form: Option<Entity<CreateSkillForm>>,
    pub plus_bounds: BoundsCell,
}

fn picker_props(props: &ComposerProps) -> ModelPickerProps {
    ModelPickerProps {
        harness: Some(props.harness),
        model: props.model.clone(),
        values: props.model_settings.clone(),
        project: Some(props.cwd.clone()),
        hide_settings: props.model_controls_beside,
        allowed_harnesses: props.allowed_model_harnesses.clone(),
        hotkeys: props.hotkeys && props.enabled,
    }
}

impl BottomBar {
    pub fn new(
        host: &Rc<dyn ComposerHost>,
        props: &ComposerProps,
        window: &mut Window,
        cx: &mut Context<Composer>,
    ) -> Self {
        let composer = cx.entity().downgrade();
        let source = host.model_source(cx);
        let model_picker = source.clone().map(|source| {
            let prefs = host.model_prefs(cx);
            let projects = host.project_providers(cx);
            let props = picker_props(props);
            let (change, settings, close, favorites) = (
                composer.clone(),
                composer.clone(),
                composer.clone(),
                composer.clone(),
            );
            cx.new(|cx| {
                ModelPicker::new(props, source, prefs, projects, window, cx)
                    .on_change(move |harness, model, _, cx| {
                        let model = model.to_string();
                        change
                            .update(cx, |_, cx| {
                                cx.emit(ComposerEvent::ModelChange { harness, model })
                            })
                            .ok();
                    })
                    .on_settings_change(move |values, _, cx| {
                        let values = values.clone();
                        settings
                            .update(cx, |_, cx| {
                                cx.emit(ComposerEvent::ModelSettingsChange(values))
                            })
                            .ok();
                    })
                    .on_close(move |window, cx| refocus(&close, window, cx))
                    .on_favorites_change(move |next, _, cx| {
                        let next = next.to_vec();
                        favorites
                            .update(cx, |_, cx| cx.emit(ComposerEvent::FavoritesChange(next)))
                            .ok();
                    })
            })
        });
        let model_pills = source.clone().map(|source| {
            let (settings, close) = (composer.clone(), composer.clone());
            let (harness, model, values) = (
                props.harness,
                props.model.clone(),
                props.model_settings.clone(),
            );
            cx.new(|cx| {
                ModelControlPills::new(harness, model, values, source, cx)
                    .on_settings_change(move |values, _, cx| {
                        let values = values.clone();
                        settings
                            .update(cx, |_, cx| {
                                cx.emit(ComposerEvent::ModelSettingsChange(values))
                            })
                            .ok();
                    })
                    .on_close(move |window, cx| refocus(&close, window, cx))
            })
        });
        let (mode, close) = (composer.clone(), composer.clone());
        let runtime_mode = props.runtime_mode;
        let busy = props.busy;
        let access = cx.new(|cx| {
            let mut picker = AccessPicker::new(runtime_mode, cx)
                .on_change(move |value, _, cx| {
                    mode.update(cx, |_, cx| cx.emit(ComposerEvent::RuntimeModeChange(value)))
                        .ok();
                })
                .on_close(move |window, cx| refocus(&close, window, cx));
            picker.set_busy(busy, cx);
            picker
        });
        Self {
            model_picker,
            model_pills,
            access,
            folder_picker: None,
            mcp_picker: None,
            create_form: None,
            plus_bounds: BoundsCell::default(),
        }
    }

    /// Pushes new props into the picker entities.
    pub fn sync(&mut self, props: &ComposerProps, cx: &mut Context<Composer>) {
        if let Some(picker) = &self.model_picker {
            let next = picker_props(props);
            picker.update(cx, |picker, cx| picker.set_props(next, cx));
        }
        if let Some(pills) = &self.model_pills {
            let (harness, model, values) = (
                props.harness,
                props.model.clone(),
                props.model_settings.clone(),
            );
            pills.update(cx, |pills, cx| {
                pills.set_selection(harness, model, values, cx)
            });
        }
        let (value, busy) = (props.runtime_mode, props.busy);
        self.access.update(cx, |access, cx| {
            access.set_value(value, cx);
            access.set_busy(busy, cx);
        });
    }

    pub fn any_open(&self, cx: &App) -> bool {
        self.model_picker.as_ref().is_some_and(|picker| {
            picker.read(cx).is_open() || picker.read(cx).is_recent_menu_open()
        }) || self.access.read(cx).is_open()
    }

    pub fn open_folder_picker(
        &mut self,
        folders: &[super::super::host::SessionFolder],
        window: &mut Window,
        cx: &mut Context<Composer>,
    ) {
        let composer = cx.entity().downgrade();
        let rows = folder_rows(folders);
        let (pick, dismiss) = (composer.clone(), composer);
        self.folder_picker = Some(cx.new(|cx| {
            SessionFolderPicker::new(rows, window, cx)
                .on_pick(move |target, window, cx| {
                    let target = match target {
                        SessionFolderTarget::Existing { folder_id } => FolderTarget::Existing {
                            folder_id: folder_id.clone(),
                        },
                        SessionFolderTarget::New { name } => {
                            FolderTarget::New { name: name.clone() }
                        }
                    };
                    pick.update(cx, |this, cx| this.pick_session_folder(target, window, cx))
                        .ok();
                })
                .on_dismiss(move |window, cx| {
                    dismiss
                        .update(cx, |this, cx| {
                            this.dismiss_session_folder_picker(window, cx)
                        })
                        .ok();
                })
        }));
    }

    pub fn open_mcp_picker(
        &mut self,
        servers: super::super::host::McpServers,
        harness: HarnessId,
        window: &mut Window,
        cx: &mut Context<Composer>,
    ) {
        let composer = cx.entity().downgrade();
        let rows_servers = servers.clone();
        let rows_for = move |query: &str| mcp_rows(&rows_servers, harness, query);
        let (pick, manage, dismiss) = (composer.clone(), composer.clone(), composer);
        let pick_servers = servers.servers.clone();
        let picker = cx.new(|cx| {
            let mut picker = McpServerPicker::new(rows_for, window, cx)
                .on_pick(move |row, window, cx| {
                    let Some(server) = pick_servers
                        .iter()
                        .find(|server| mcp_key(server) == row.key.as_ref())
                        .cloned()
                    else {
                        return;
                    };
                    pick.update(cx, |this, cx| this.pick_mcp_server(&server, window, cx))
                        .ok();
                })
                .on_manage(move |_, cx| {
                    manage.update(cx, |this, cx| this.manage_mcp(cx)).ok();
                })
                .on_dismiss(move |reason, window, cx| {
                    dismiss
                        .update(cx, |this, cx| {
                            this.dismiss_mcp_picker(reason == DismissReason::Escape, window, cx)
                        })
                        .ok();
                });
            picker.set_loading(servers.loading, cx);
            picker.set_error(servers.error.clone(), cx);
            picker
        });
        self.mcp_picker = Some(picker);
    }

    pub fn close_mcp_picker(&mut self, _cx: &mut Context<Composer>) {
        self.mcp_picker = None;
    }
}

fn refocus(composer: &WeakEntity<Composer>, window: &mut Window, cx: &mut App) {
    composer.update(cx, |this, cx| this.focus(window, cx)).ok();
}

fn mcp_key(server: &super::super::model::mcp::McpConnection) -> String {
    format!(
        "{}:{}:{}:{}",
        server.provider, server.scope, server.config_path, server.name
    )
}

/// `mcpPickerServers` as picker rows.
fn mcp_rows(
    servers: &super::super::host::McpServers,
    harness: HarnessId,
    query: &str,
) -> Vec<McpServerRow> {
    mcp_picker_servers(
        &servers.servers,
        harness.as_str(),
        &servers.claude_status,
        query,
    )
    .into_iter()
    .map(|row| McpServerRow {
        key: mcp_key(&row.server).into(),
        name: row.server.name.clone().into(),
        icon: match row.server.provider.as_str() {
            "claude" | "claude_desktop" => HarnessId::Claude,
            other => HarnessId::parse(other).unwrap_or(HarnessId::Claude),
        },
        provider_label: mcp_provider_label(&row.server.provider).to_string().into(),
        scope: row.server.scope.clone().into(),
        availability: match row.availability {
            McpAvailability::Available => crate::pickers::McpAvailability::Available,
            McpAvailability::Authentication => crate::pickers::McpAvailability::Authentication,
            McpAvailability::Unavailable => crate::pickers::McpAvailability::Unavailable,
        },
        detail: row.detail.into(),
    })
    .collect()
}

/// A slash row as the skill picker shows it.
fn picker_skill(skill: &Skill) -> PickerSkill {
    use crate::pickers::skill_picker::{SkillKind as PickerKind, SkillScope as PickerScope};
    PickerSkill {
        kind: match skill.kind {
            SkillKind::File => PickerKind::File,
            SkillKind::Builtin => PickerKind::Builtin,
            SkillKind::Native => PickerKind::Native,
        },
        name: skill.name.clone().into(),
        invocation: skill.invocation.clone().into(),
        description: skill.description.clone().into(),
        source: skill.source.clone().into(),
        scope: match skill.scope.as_str() {
            "project" => PickerScope::Project,
            "user" => PickerScope::User,
            _ => PickerScope::Builtin,
        },
        origin: None,
        input_hint: None,
        subcommands: Vec::new(),
    }
}

impl Composer {
    fn tool_button(
        &self,
        id: &'static str,
        name: IconName,
        active: bool,
        theme: &Theme,
    ) -> gpui::Stateful<gpui::Div> {
        let c = theme.colors;
        let (bg, ink) = if active {
            (c.selection_emphasis, c.content)
        } else {
            (c.selection, theme.content(0.50))
        };
        let (hover_bg, hover_ink) = (c.selection_hover, c.content);
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(u(26.))
            .rounded(u(theme.radius.md))
            .bg(bg)
            .when(!active, |el| {
                el.hover(move |style| style.bg(hover_bg).text_color(hover_ink))
            })
            .child(icon(name).size(u(14.)).text_color(ink))
    }

    /// The + menu.
    fn render_plus_menu(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let supported = self.attachments_supported();
        let remote = self.remote();
        let hover = theme.content(0.10);
        let row = |id: &'static str,
                   glyph: AnyElement,
                   label: AnyElement,
                   description: SharedString,
                   check: Option<AnyElement>,
                   disabled: bool| {
            div()
                .id(id)
                .flex()
                .w_full()
                .items_start()
                .gap(u(10.))
                .rounded(u(theme.radius.lg))
                .px(u(8.))
                .py(u(8.))
                .text_color(theme.colors.content)
                .when(disabled, |el| el.opacity(0.4))
                .when(!disabled, |el| el.hover(move |style| style.bg(hover)))
                .child(div().mt(u(2.)).flex_none().child(glyph))
                .child(
                    div().min_w_0().flex_1().child(label).child(
                        div()
                            .truncate()
                            .text_px(11.)
                            .line_height(u(16.))
                            .text_color(theme.content(0.45))
                            .child(description),
                    ),
                )
                .children(check.map(|check| div().mt(u(2.)).flex_none().child(check)))
        };
        let label = |text: &'static str| div().text_px(13.).child(text).into_any_element();
        let upload_description: SharedString = if supported {
            "Attach files or images".into()
        } else if remote && !self.props.remote_features.is_some_and(|f| f.attachments) {
            "Update this machine’s host to attach files".into()
        } else {
            format!(
                "{} does not support attachments",
                self.props.harness.title()
            )
            .into()
        };
        let mut rows: Vec<AnyElement> = vec![
            row(
                "plus-upload",
                icon(IconName::FilePlus)
                    .size(u(16.))
                    .text_color(theme.colors.content)
                    .into_any_element(),
                label("Upload file"),
                upload_description,
                None,
                !supported,
            )
            .when(supported, |el| {
                el.on_click(cx.listener(|this, _, window, cx| {
                    this.plus_open = false;
                    this.attach_from_picker(window, cx);
                    cx.notify();
                }))
            })
            .into_any_element(),
        ];
        let mut modes = Vec::new();
        if !remote || self.props.remote_features.is_some_and(|f| f.plan) {
            modes.push(Mode::Plan);
        }
        if !remote {
            modes.push(Mode::Operator);
        }
        if !remote && !self.props.hide_top_bar {
            modes.push(Mode::Orchestrator);
        }
        if self.props.can_save_draft {
            modes.push(Mode::Draft);
        }
        for mode in modes {
            let menu = mode.menu().expect("menu row");
            let active = self.mode_active(mode);
            let id: &'static str = match mode {
                Mode::Plan => "plus-plan",
                Mode::Operator => "plus-operator",
                Mode::Orchestrator => "plus-orchestrator",
                _ => "plus-draft",
            };
            let title = if mode == Mode::Orchestrator {
                let (bg, ink) = orchestrator_badge();
                div()
                    .flex()
                    .items_center()
                    .gap(u(6.))
                    .child(div().text_px(13.).child(menu.label))
                    .child(
                        div()
                            .rounded_full()
                            .bg(bg)
                            .px(u(6.))
                            .py(u(2.))
                            .text_px(9.)
                            .medium()
                            .line_height(relative(1.))
                            .text_color(ink)
                            .child("v1"),
                    )
                    .into_any_element()
            } else {
                label(menu.label)
            };
            let check = active.then(|| {
                icon(IconName::Check)
                    .size(u(14.))
                    .text_color(mode_menu_check(mode, &theme))
                    .into_any_element()
            });
            rows.push(
                row(
                    id,
                    icon(mode.icon())
                        .size(u(16.))
                        .text_color(mode_menu_icon(mode, &theme))
                        .into_any_element(),
                    title,
                    menu.description.into(),
                    check,
                    false,
                )
                .on_click(
                    cx.listener(move |this, _, window, cx| this.toggle_mode(mode, window, cx)),
                )
                .into_any_element(),
            );
        }
        let content = div()
            .p(u(6.))
            .child(
                div()
                    .px(u(8.))
                    .pb(u(4.))
                    .pt(u(2.))
                    .text_px(10.)
                    .medium()
                    .text_color(theme.content(0.40))
                    .child("ADD TO MESSAGE"),
            )
            .children(rows);
        let composer = cx.entity().downgrade();
        let plus = self.bar.plus_bounds.clone();
        let surface = popover_surface(
            "composer-plus",
            Some(250.),
            None,
            move |event, _, cx| {
                if plus.contains(event.position) {
                    return;
                }
                composer
                    .update(cx, |this, cx| {
                        this.plus_open = false;
                        cx.notify();
                    })
                    .ok();
            },
            content,
        );
        anchored_popover(Side::Top, 6., theme.layer.popover, window, surface)
    }

    fn render_mode_pills(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.props.compact {
            return Vec::new();
        }
        let theme = Theme::of(cx).clone();
        [Mode::Operator, Mode::Orchestrator, Mode::Plan, Mode::Draft]
            .into_iter()
            .filter(|mode| self.mode_active(*mode))
            .filter_map(|mode| {
                let pill = mode.pill()?;
                let colors = mode_pill(mode, &theme);
                let (hover_bg, hover_ink) = (colors.hover, colors.hover_text);
                let id: &'static str = match mode {
                    Mode::Plan => "pill-plan",
                    Mode::Operator => "pill-operator",
                    Mode::Orchestrator => "pill-orchestrator",
                    _ => "pill-draft",
                };
                Some(
                    div()
                        .id(id)
                        .flex()
                        .flex_none()
                        .h(u(26.))
                        .items_center()
                        .gap(u(4.))
                        .rounded(u(theme.radius.md))
                        .px(u(6.))
                        .text_px(11.)
                        .bg(colors.bg)
                        .text_color(colors.text)
                        .when(mode == Mode::Operator || mode == Mode::Orchestrator, |el| {
                            el.medium()
                        })
                        .when_some(colors.dashed_border, |el, border| {
                            el.border_1().border_dashed().border_color(border)
                        })
                        .hover(move |style| style.bg(hover_bg).text_color(hover_ink))
                        .tooltip(monocode_ui::widgets::tooltip(format!(
                            "Turn off {}",
                            pill.title
                        )))
                        .on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.clear_mode(mode, window, cx)
                            }),
                        )
                        .child(icon(mode.icon()).size(u(14.)).text_color(colors.text))
                        .child(pill.label)
                        .child(icon(IconName::X).size(u(12.)).text_color(colors.text))
                        .into_any_element(),
                )
            })
            .collect()
    }

    /// A stand-in for the model picker when the host gave no model source.
    fn render_model_placeholder(&self, theme: &Theme) -> AnyElement {
        let logo =
            ProviderLogo::from_id(self.props.harness.as_str()).unwrap_or(ProviderLogo::Claude);
        let label = if self.props.model.is_empty() {
            self.props.harness.title().to_string()
        } else {
            self.props.model.clone()
        };
        div()
            .flex()
            .flex_none()
            .h(u(26.))
            .max_w(u(160.))
            .items_center()
            .gap(u(4.))
            .rounded(u(theme.radius.md))
            .px(u(6.))
            .bg(theme.colors.selection)
            .text_color(theme.colors.content)
            .child(provider_logo(logo).size(16.))
            .child(div().min_w_0().truncate().text_px(11.).child(label))
            .child(
                icon(IconName::ChevronDown)
                    .size(u(12.))
                    .text_color(theme.content(0.50)),
            )
            .into_any_element()
    }

    /// `ComposerAction`: Send, or Stop while a turn runs.
    fn render_action(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let label: &'static str = if self.draft_active() {
            "Save draft"
        } else {
            "Send"
        };
        let has_value = self.has_value && !self.props.worktree_removed;
        let base = |id: &'static str| {
            div()
                .id(id)
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(u(26.))
                .rounded(u(theme.radius.md))
        };
        let send = |enabled: bool| {
            let (bg, ink) = if enabled {
                (c.primary, c.primary_foreground)
            } else {
                (c.primary_disabled, c.primary_disabled_foreground)
            };
            let hover = c.primary_hover;
            base("composer-send")
                .bg(bg)
                .when(enabled, |el| el.hover(move |style| style.bg(hover)))
                .tooltip(monocode_ui::widgets::tooltip(label))
                .child(icon(IconName::ArrowUp).size(u(14.)).text_color(ink))
        };
        if self.props.disabled {
            return send(false).into_any_element();
        }
        if self.props.busy && !(has_value && self.props.allow_busy_submit) {
            let (bg, hover, ink) = stop_button();
            return base("composer-stop")
                .bg(bg)
                .hover(move |style| style.bg(hover))
                .tooltip(monocode_ui::widgets::tooltip("Stop"))
                .on_click(cx.listener(|this, _, window, cx| {
                    let host = this.host.clone();
                    host.stop(window, cx);
                }))
                .child(div().size(u(10.)).rounded(u(2.)).bg(ink))
                .into_any_element();
        }
        let enabled = has_value;
        send(enabled)
            .when(enabled, |el| {
                el.on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)))
            })
            .into_any_element()
    }

    fn render_cancel_edit(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let accent = theme.user_accent_or_accent();
        let ink = theme.colors.content;
        let hover_bg = theme.content(0.15);
        let hover_border = gpui::Hsla { a: 0.35, ..accent };
        div()
            .id("composer-cancel-edit")
            .flex()
            .flex_none()
            .h(u(26.))
            .items_center()
            .gap(u(4.))
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(gpui::Hsla { a: 0.20, ..accent })
            .bg(gpui::Hsla { a: 0.10, ..accent })
            .px(u(8.))
            .text_px(11.)
            .medium()
            .text_color(accent)
            .hover(move |style| {
                style
                    .bg(hover_bg)
                    .text_color(ink)
                    .border_color(hover_border)
            })
            .tooltip(monocode_ui::widgets::tooltip("Stop editing last message"))
            .on_click(cx.listener(|this, _, window, cx| this.exit_edit_mode(window, cx)))
            .child(icon(IconName::X).size(u(12.)).text_color(accent))
            .child("Cancel edit")
            .into_any_element()
    }

    fn render_bottom_bar(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let mut bar = div().flex().items_center().gap(u(4.)).px(u(8.)).pb(u(8.));
        if !self.props.compact {
            let mut plus = div()
                .relative()
                .flex_none()
                .child(self.bar.plus_bounds.probe())
                .child(
                    self.tool_button(
                        "composer-plus-button",
                        IconName::Plus,
                        self.plus_open,
                        &theme,
                    )
                    .tooltip(monocode_ui::widgets::tooltip("Add files or choose a mode"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.plus_open = !this.plus_open;
                        cx.notify();
                    })),
                );
            if self.plus_open {
                plus = plus.child(self.render_plus_menu(window, cx));
            }
            bar = bar.child(plus);
        }
        bar = bar.children(self.render_mode_pills(cx));
        let mut tools = div().flex().flex_none().items_center().gap(u(4.));
        tools = match &self.bar.model_picker {
            Some(picker) => tools.child(picker.clone()),
            None => tools.child(self.render_model_placeholder(&theme)),
        };
        if self.props.model_controls_beside
            && let Some(pills) = &self.bar.model_pills
            && !pills.read(cx).pills().is_empty()
        {
            tools = tools.child(pills.clone());
        }
        if !self.props.compact && self.props.harness != HarnessId::Fx {
            tools = tools.child(self.bar.access.clone());
        }
        bar = bar.child(
            div()
                .flex()
                .min_w_0()
                .flex_1()
                .items_center()
                .overflow_hidden()
                .child(tools),
        );
        if self.resend_edited {
            bar = bar.child(self.render_cancel_edit(cx));
        }
        bar.child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(u(4.))
                .child(self.render_action(cx)),
        )
        .into_any_element()
    }

    fn render_top_bar(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.props.hide_top_bar {
            return None;
        }
        let theme = Theme::of(cx).clone();
        let label = |glyph: IconName, text: String| {
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(u(6.))
                .text_color(theme.content(0.50))
                .child(icon(glyph).size(u(14.)).text_color(theme.content(0.50)))
                .child(
                    div()
                        .truncate()
                        .font_family(theme.fonts.mono.clone())
                        .text_px(12.)
                        .child(text),
                )
        };
        let mut row = div()
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(10.))
            .overflow_hidden()
            .px(u(12.))
            .pt(u(10.));
        if !self.top_bar_views.is_empty() {
            row = row.children(self.top_bar_views.iter().cloned());
        } else {
            if !self.remote() && !self.props.hide_project_picker {
                row = row.child(label(IconName::Folder, pretty_cwd(&self.props.cwd)));
            }
            if !self.props.hide_branch_picker
                && let Some(branch) = self.props.branch.clone()
            {
                row = row.child(label(IconName::GitBranch, branch));
            }
        }
        row = row.child(
            div()
                .ml_auto()
                .flex()
                .flex_none()
                .items_center()
                .children(self.render_context_meter(window, cx)),
        );
        Some(row.into_any_element())
    }

    /// The slash, `@`, MCP, and folder pickers above the box.
    fn render_pickers(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let composer = cx.entity().downgrade();
        let picker: AnyElement = if self.mcp_picker_open {
            self.bar.mcp_picker.clone()?.into_any_element()
        } else if self.session_folder_open {
            self.bar.folder_picker.clone()?.into_any_element()
        } else if self.skill_picker_open() {
            let ranked = self.ranked_skills();
            let rows: Vec<PickerSkill> = ranked.iter().map(picker_skill).collect();
            let (active, pick, start) = (composer.clone(), composer.clone(), composer.clone());
            let query = self
                .slash
                .as_ref()
                .map(|t| t.query.clone())
                .unwrap_or_default();
            skill_picker("composer-skill-picker", rows, query, self.skill_active)
                .creating(self.bar.create_form.clone().filter(|_| self.creating_skill))
                .on_active(move |index, _, cx| {
                    active
                        .update(cx, |this, cx| {
                            this.skill_active = index;
                            cx.notify();
                        })
                        .ok();
                })
                .on_pick(move |row, window, cx| {
                    let skill = ranked
                        .iter()
                        .find(|skill| {
                            skill.invocation == row.invocation.as_ref()
                                && picker_skill(skill).key() == row.key()
                        })
                        .cloned();
                    if let Some(skill) = skill {
                        pick.update(cx, |this, cx| this.pick_skill(&skill, window, cx))
                            .ok();
                    }
                })
                .on_start_create(move |window, cx| {
                    start
                        .update(cx, |this, cx| this.open_create_form(window, cx))
                        .ok();
                })
                .into_any_element()
        } else if self.mention_open() {
            let files: Vec<MentionFile> = self
                .ranked_files
                .iter()
                .map(|ranked| MentionFile {
                    path: ranked.file.path.clone().into(),
                    relative: ranked.file.relative.clone().into(),
                    name: ranked.file.name.clone().into(),
                    is_dir: ranked.file.is_dir,
                    positions: ranked.positions.clone(),
                })
                .collect();
            let ranked = self.ranked_files.clone();
            let (active, pick) = (composer.clone(), composer.clone());
            let query = self
                .mention
                .as_ref()
                .map(|t| t.query.clone())
                .unwrap_or_default();
            let cwd = self.props.execution_cwd.clone();
            let loading = super::super::model::paths::looks_like_project(&cwd)
                && self.host.mentions_loading(&cwd, cx);
            file_mention_picker("composer-mention-picker", files, query, self.mention_active)
                .loading(loading)
                .include_notes(self.props.notes_enabled)
                .on_active(move |index, _, cx| {
                    active
                        .update(cx, |this, cx| {
                            this.mention_active = index;
                            cx.notify();
                        })
                        .ok();
                })
                .on_pick(move |row, window, cx| {
                    let file = ranked
                        .iter()
                        .find(|ranked| ranked.file.path == row.path.as_ref())
                        .map(|ranked| ranked.file.clone());
                    if let Some(file) = file {
                        pick.update(cx, |this, cx| this.pick_mention(&file, window, cx))
                            .ok();
                    }
                })
                .into_any_element()
        } else {
            return None;
        };
        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(relative(1.))
                .pb(u(4.))
                .child(picker)
                .into_any_element(),
        )
    }

    /// SkillPicker's create row: the starter-skill form.
    pub(crate) fn open_create_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self
            .slash
            .as_ref()
            .map(|t| t.query.clone())
            .unwrap_or_default();
        let project = !self.remote()
            && super::super::model::paths::looks_like_project(&self.props.execution_cwd);
        let composer = cx.entity().downgrade();
        let (cancel, create) = (composer.clone(), composer);
        self.bar.create_form = Some(cx.new(|cx| {
            CreateSkillForm::new(&query, project, window, cx)
                .on_cancel(move |window, cx| {
                    cancel
                        .update(cx, |this, cx| this.cancel_create_skill(window, cx))
                        .ok();
                })
                .on_create(move |name, scope, window, cx| {
                    let scope = match scope {
                        CreateScope::Project => NewSkillScope::Project,
                        CreateScope::User => NewSkillScope::User,
                    };
                    let name = name.to_string();
                    create
                        .update(cx, |this, cx| this.create_skill(name, scope, window, cx))
                        .ok();
                })
        }));
        self.start_create_skill(cx);
    }
}

impl Render for Composer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.file_drag && !cx.has_active_drag() {
            self.file_drag = false;
        }
        // The placeholder follows the chips and cards (it notifies only on
        // a change, so this does not loop).
        let placeholder = self.placeholder_text();
        self.prompt
            .update(cx, |prompt, cx| prompt.set_placeholder(placeholder, cx));
        if let Some(form) = self.bar.create_form.clone() {
            let (busy, error) = (self.create_busy, self.create_error.clone());
            form.update(cx, |form, cx| {
                form.set_busy(busy, cx);
                form.set_error(error.map(SharedString::from), cx);
            });
        }
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let focused = self.prompt.read(cx).is_focused(window);
        let accent = theme.user_accent_or_accent();
        let border = if self.file_drag {
            theme.accent(0.60)
        } else if self.resend_edited {
            gpui::Hsla { a: 0.32, ..accent }
        } else if focused {
            theme.content(0.20)
        } else {
            theme.content(0.10)
        };
        let geometry = self.runner_geometry.r#box.clone();
        let mut r#box = div()
            .id("composer-box")
            .relative()
            .flex()
            .flex_col()
            .rounded(u(theme.radius.lg))
            .border_1()
            .border_color(border)
            .when(self.resend_edited && !self.file_drag, |el| {
                el.border_dashed()
            })
            .child(
                canvas(
                    move |bounds, _, _| geometry.set(Some(bounds)),
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
        r#box = if theme.is_dark() {
            r#box.child(glass_backdrop(theme.radius.lg, 4., theme.content(0.03)))
        } else {
            r#box.bg(c.background_base).shadow_md()
        };
        r#box = r#box.children(self.render_top_bar(window, cx));
        if !self.context_items.is_empty() || !self.attachments.is_empty() {
            let items = self.context_items.clone();
            let mut chips = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(u(6.))
                .px(u(12.))
                .pt(u(8.));
            for item in &items {
                chips = chips.child(self.render_context_chip(item, window, cx));
            }
            for file in self.attachments.clone() {
                chips = chips.child(self.render_attachment_chip(&file, cx));
            }
            r#box = r#box.child(chips);
        }
        if let Some(error) = self.paste_error.clone() {
            r#box = r#box.child(
                div()
                    .px(u(12.))
                    .pt(u(8.))
                    .text_px(12.)
                    .text_color(c.danger)
                    .child(error),
            );
        }
        r#box = r#box
            .children(self.card_views.iter().cloned())
            .child(
                div()
                    .relative()
                    .text_size(u(14.))
                    .line_height(u(22.))
                    .text_color(c.content)
                    .font_family(theme.fonts.sans.clone())
                    .child(self.prompt.clone()),
            )
            .child(self.render_bottom_bar(window, cx));
        if self.file_drag {
            r#box = r#box.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.lg))
                    .bg(theme.accent(0.08))
                    .text_px(12.)
                    .text_color(theme.content(0.70))
                    .child("Drop files to attach"),
            );
        }
        let mut wrapper = div().relative().flex().flex_col().child(r#box);
        wrapper = wrapper.children(self.render_pickers(cx));
        if let Some(runner) = self.runner.clone() {
            wrapper = wrapper.child(runner);
        }
        let ledge = self.runner_geometry.ledge.clone();
        let queue = self.render_message_queue(window, cx).map(|queue| {
            div().relative().child(queue).child(
                canvas(move |bounds, _, _| ledge.set(Some(bounds)), |_, _, _, _| {})
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            )
        });
        if queue.is_none() {
            self.runner_geometry.ledge.set(None);
        }
        let disabled = self.props.disabled;
        div()
            .id("composer")
            .key_context("Composer")
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .font_family(theme.fonts.sans.clone())
            .when(!self.props.shell && !self.props.compact, |el| {
                el.px(u(6.)).pb(u(6.))
            })
            .when(!disabled, |el| {
                el.capture_action(
                    cx.listener(|this, action: &Enter, window, cx| {
                        this.on_enter(action, window, cx)
                    }),
                )
                .capture_action(cx.listener(|this, action: &MoveUp, window, cx| {
                    this.on_move_up(action, window, cx)
                }))
                .capture_action(cx.listener(|this, action: &MoveDown, window, cx| {
                    this.on_move_down(action, window, cx)
                }))
                .capture_action(
                    cx.listener(|this, action: &Tab, window, cx| this.on_tab(action, window, cx)),
                )
                .capture_action(cx.listener(|this, action: &Escape, window, cx| {
                    this.on_escape(action, window, cx)
                }))
                .capture_action(
                    cx.listener(|this, action: &Paste, window, cx| {
                        this.on_paste(action, window, cx)
                    }),
                )
                .capture_key_down(
                    cx.listener(|this, event, window, cx| this.on_key_down(event, window, cx)),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|_, _, _, cx| cx.emit(ComposerEvent::Focus)),
                )
                .on_drag_move::<ExternalPaths>(cx.listener(
                    |this, event: &DragMoveEvent<ExternalPaths>, _, cx| {
                        let over = event.bounds.contains(&event.event.position);
                        this.set_file_drag(over, cx);
                    },
                ))
                .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                    this.on_external_drop(paths, window, cx)
                }))
            })
            .children(self.header_views.iter().cloned())
            .children(queue)
            .child(wrapper)
            .children(self.render_attachment_preview(window, cx))
    }
}

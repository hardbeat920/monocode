use super::*;
use crate::popover::MenuOpened;
use gpui::App;
use monocode_core::{
    accounts::Profile,
    response::{Target, TransferKind},
};
use std::collections::{HashMap, HashSet};

type Catalogs = Vec<(Kind, Option<Result<Vec<Model>, String>>)>;

pub(super) struct ResponseMenu {
    pub(super) menu: Entity<Menu>,
    pub(super) anchor: AnchorSlot,
    pub(super) _events: Vec<Subscription>,
    pub(super) regions: Vec<AnchorSlot>,
    models: HashMap<Kind, Entity<Menu>>,
    profiles: HashMap<Kind, Profile>,
    requested: HashSet<Kind>,
}

impl Conversation {
    pub(super) fn open_response_menu(
        &mut self,
        turn: usize,
        kind: TransferKind,
        token: Instant,
        anchor: AnchorSlot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.make_response_menu(turn, kind, anchor.clone(), None, window, cx);
        let remote = self.is_remote().then(|| self.summary.cwd.clone());
        let hidden = cx
            .try_global::<crate::settings::Preferences>()
            .map(|prefs| prefs.hidden_for(&self.summary.cwd))
            .unwrap_or_default();
        let work = cx.background_executor().spawn(async move {
            if let Some(project) = remote {
                return catalog::remote_catalog(&project).map(|mut catalog| {
                    catalog
                        .providers
                        .into_iter()
                        .map(|provider| {
                            let rows = if let Some(error) = catalog.errors.remove(&provider) {
                                Err(error)
                            } else {
                                Ok(catalog.models.remove(&provider).unwrap_or_default())
                            };
                            (provider, Some(rows))
                        })
                        .collect()
                });
            }
            // Discover executables only. Starting every CLI here made one slow
            // provider delay the entire picker, including cached catalogs.
            Ok(Kind::ALL
                .into_iter()
                .filter(|provider| !hidden.iter().any(|id| id == provider.id()))
                .filter(|provider| monocode_core::provider::resolve(*provider).is_some())
                .map(|provider| (provider, None))
                .collect::<Catalogs>())
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.response_request != Some((turn, kind, token)) {
                    return;
                }
                match result {
                    Ok(catalogs) => {
                        this.make_response_menu(turn, kind, anchor, Some(catalogs), window, cx)
                    }
                    Err(error) => {
                        if let Some(menu) = &this.response_menu {
                            menu.menu.update(cx, |menu, cx| {
                                menu.set_items(vec![MenuItem::Note(error.into())], cx)
                            });
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn make_response_menu(
        &mut self,
        turn: usize,
        kind: TransferKind,
        anchor: AnchorSlot,
        catalogs: Option<Catalogs>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let from = self.response_provider(turn, cx);
        let label = if kind == TransferKind::Handoff {
            "Hand this session to another agent"
        } else {
            "Send this turn to another agent"
        };
        let mut items = vec![MenuItem::Note(
            if kind == TransferKind::Handoff {
                "Hand this session to another agent to continue the work."
            } else {
                "Send this turn to another agent to review the work."
            }
            .into(),
        )];
        if catalogs.is_none() {
            items.push(MenuItem::Note("Loading providers…".into()));
        }
        let mut events = Vec::new();
        let mut regions = Vec::new();
        let mut menus = HashMap::new();
        let mut profiles = HashMap::new();
        let mut requested = HashSet::new();
        for (provider, initial) in catalogs.into_iter().flatten() {
            if kind == TransferKind::Handoff && provider.id() == from {
                continue;
            }
            let profile = if provider.id() == self.summary.provider {
                self.settings.account.clone()
            } else {
                crate::accounts::get(cx)
                    .map(|center| center.read(cx).selected(provider, &self.summary.cwd))
                    .unwrap_or_default()
            };
            if initial.is_some() {
                requested.insert(provider);
            }
            let cached = initial.or_else(|| {
                (!self.is_remote())
                    .then(|| catalog::cached_models_for(provider, &profile))
                    .flatten()
                    .map(Ok)
            });
            let options = cached
                .map(|result| self.response_model_items(turn, kind, provider, result, cx))
                .unwrap_or_else(|| vec![MenuItem::Note("Loading models…".into())]);
            let models = cx.new(|cx| {
                Menu::embedded(provider.label(), options, cx)
                    .width(240.)
                    .style(MenuStyle::Settings)
            });
            regions.push(models.read(cx).submenu_region());
            let surface = models.read(cx).surface_region();
            let focus = models.focus_handle(cx);
            events.push(cx.subscribe_in(
                &models,
                window,
                move |this, _, event: &MenuEvent, window, cx| match event {
                    MenuEvent::Picked(id, _) => this.finish_response_target(turn, kind, id, cx),
                    MenuEvent::Dismissed => {
                        this.close_response_menu(cx);
                        window.focus(&this.input.focus_handle(cx), cx);
                    }
                },
            ));
            menus.insert(provider, models.clone());
            profiles.insert(provider, profile);
            items.push(
                MenuAction::new(provider.id(), provider.label())
                    .image(provider.id())
                    .flyout(Flyout {
                        view: models.into(),
                        width: 240.,
                        padding: 4.,
                        height: None,
                        max_height: Some(288.),
                        focus: Some(focus),
                        surface: Some(surface),
                    })
                    .into(),
            );
        }
        if items.len() == 1 {
            items.push(MenuItem::Note("No other providers are available.".into()));
        }
        let menu = cx.new(|cx| {
            Menu::new(label, items, window, cx)
                .width(240.)
                .style(MenuStyle::Settings)
        });
        events.push(cx.subscribe_in(
            &menu,
            window,
            move |this, _, event: &MenuEvent, window, cx| match event {
                MenuEvent::Picked(id, _) => this.finish_response_target(turn, kind, id, cx),
                MenuEvent::Dismissed => {
                    this.close_response_menu(cx);
                    window.focus(&this.input.focus_handle(cx), cx);
                }
            },
        ));
        events.push(cx.subscribe(&menu, move |this, _, event: &MenuOpened, cx| {
            if let Some(provider) = Kind::parse(&event.0) {
                this.load_response_models(provider, cx);
            }
        }));
        self.response_menu = Some(ResponseMenu {
            menu,
            anchor,
            _events: events,
            regions,
            models: menus,
            profiles,
            requested,
        });
    }

    fn load_response_models(&mut self, provider: Kind, cx: &mut Context<Self>) {
        let Some(menu) = &mut self.response_menu else {
            return;
        };
        if !menu.requested.insert(provider) {
            return;
        }
        let Some(models) = menu.models.get(&provider).cloned() else {
            return;
        };
        // UI tests drive completion explicitly, without starting real CLIs.
        if cfg!(test) {
            return;
        }
        let profile = menu.profiles.get(&provider).cloned().unwrap_or_default();
        let request = self.response_request;
        let work = cx
            .background_executor()
            .spawn(async move { catalog::models_for(provider, &profile) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.finish_response_models(request, provider, &models, result, cx);
            });
        })
        .detach();
    }

    fn finish_response_models(
        &mut self,
        request: Option<(usize, TransferKind, Instant)>,
        provider: Kind,
        menu: &Entity<Menu>,
        result: Result<Vec<Model>, String>,
        cx: &mut Context<Self>,
    ) {
        // A closed/reopened picker must not receive an older request.
        let Some((turn, kind, _)) =
            request.filter(|request| self.response_request == Some(*request))
        else {
            return;
        };
        let items = self.response_model_items(turn, kind, provider, result, cx);
        menu.update(cx, |menu, cx| menu.set_items(items, cx));
        cx.notify();
    }

    fn response_provider(&self, turn: usize, cx: &App) -> String {
        self.transcript
            .read(cx)
            .turns
            .get(&turn)
            .map(|t| t.provider.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.summary.provider)
            .to_owned()
    }

    fn response_model_items(
        &self,
        turn: usize,
        kind: TransferKind,
        provider: Kind,
        result: Result<Vec<Model>, String>,
        cx: &App,
    ) -> Vec<MenuItem> {
        let models = match result {
            Ok(models) => models,
            Err(error) => {
                return vec![MenuItem::Note(
                    format!("Couldn't load models: {error}").into(),
                )];
            }
        };
        let from = self.response_provider(turn, cx);
        let transcript = self.transcript.read(cx);
        let source = transcript.turns.get(&turn);
        let hidden = cx
            .try_global::<crate::settings::Preferences>()
            .map(|prefs| prefs.hidden_models_for(&self.summary.cwd))
            .unwrap_or_default();
        let mut options = Vec::new();
        for model in models {
            if hidden
                .iter()
                .any(|c| c.provider == provider.id() && c.model.as_deref() == Some(&model.id))
            {
                continue;
            }
            if kind == TransferKind::SecondOpinion
                && provider.id() == from
                && source.is_some_and(|source| {
                    let source_id = source.model_id.as_deref().unwrap_or(&source.model);
                    let source_id = source_id
                        .strip_prefix(&format!("{from}:"))
                        .unwrap_or(source_id);
                    [
                        Some(model.id.as_str()),
                        model.alias_target.as_deref(),
                        Some(model.name.as_str()),
                        Some(model.name.strip_prefix("Claude ").unwrap_or(&model.name)),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|candidate| {
                        model_ids_match(provider, source_id, candidate)
                            || source.model.eq_ignore_ascii_case(candidate)
                    })
                })
            {
                continue;
            }
            let target = |effort: Option<String>| {
                serde_json::to_string(&Target {
                    provider: provider.id().into(),
                    model: Some(model.id.clone()),
                    effort,
                    settings: model
                        .controls
                        .iter()
                        .map(|control| (control.id.clone(), control.current.clone()))
                        .collect(),
                })
                .unwrap()
            };
            let mut option = MenuAction::new(
                target(None),
                model_display_name(provider, &model.id, &model.name),
            );
            if !model.efforts.is_empty() {
                let mut efforts = vec![MenuAction::new(target(None), "Default")];
                efforts.extend(model.efforts.iter().map(|effort| {
                    MenuAction::new(target(Some(effort.clone())), effort_name(effort))
                }));
                option = option.submenu(efforts);
            }
            options.push(option.into());
        }
        if options.is_empty() {
            options.push(MenuItem::Note("No different models are available.".into()));
        }
        options
    }
}

pub(super) fn model_ids_match(provider: Kind, left: &str, right: &str) -> bool {
    let normalize = |id: &str| {
        let id = id
            .strip_prefix(&format!("{}:", provider.id()))
            .unwrap_or(id);
        let id = id.strip_suffix("[1m]").unwrap_or(id);
        if provider == Kind::Claude {
            id.strip_prefix("claude-")
                .unwrap_or(id)
                .replace('.', "-")
                .to_lowercase()
        } else {
            id.to_lowercase()
        }
    };
    normalize(left) == normalize(right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, WindowHandle};
    use monocode_core::store::Store;
    use std::sync::Mutex;

    #[test]
    fn saved_model_ids_compare_across_claude_context_and_catalog_formats() {
        assert!(model_ids_match(
            Kind::Claude,
            "claude:opus-5.5",
            "claude-opus-5-5"
        ));
        assert!(model_ids_match(
            Kind::Claude,
            "claude-fable-5[1m]",
            "claude-fable-5"
        ));
        assert!(!model_ids_match(Kind::Claude, "opus", "claude-opus-5-5"));
        assert!(!model_ids_match(Kind::Codex, "same-5.5", "same-5-5"));
    }

    fn conversation(cx: &mut TestAppContext) -> WindowHandle<Conversation> {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let summary = store.create("claude", "/work/response-picker").unwrap();
        let window = cx.update(|cx| {
            input::bind(cx);
            crate::popover::bind(cx);
            cx.set_reduce_motion(true);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| Conversation::new(summary, Arc::new(Mutex::new(store)), cx))
            })
            .unwrap()
        });
        window
            .update(cx, |view, window, cx| {
                view.summary.has_messages = true;
                view.transcript.update(cx, |transcript, cx| {
                    transcript.patch(
                        0,
                        vec![
                            monocode_core::transcript::Row {
                                role: "user".into(),
                                text: "Review the footer".into(),
                                first: true,
                                sent_at: None,
                            },
                            monocode_core::transcript::Row {
                                role: "assistant".into(),
                                text: "The footer is ready.".into(),
                                first: true,
                                sent_at: None,
                            },
                        ],
                        cx,
                    );
                });
                window.focus(&view.input.focus_handle(cx), cx)
            })
            .unwrap();
        cx.run_until_parked();
        window
    }

    fn model(id: &str, name: &str) -> Model {
        Model {
            id: id.into(),
            name: name.into(),
            alias_target: None,
            efforts: vec!["low".into(), "high".into()],
            default_effort: Some("high".into()),
            fast: false,
            thinking: false,
            default: false,
            tiers: Vec::new(),
            controls: Vec::new(),
        }
    }

    #[gpui::test]
    fn response_picker_loads_only_the_opened_provider_and_keeps_the_flyout(
        cx: &mut TestAppContext,
    ) {
        let window = conversation(cx);
        window
            .update(cx, |view, window, cx| {
                view.response_request = Some((1, TransferKind::SecondOpinion, Instant::now()));
                view.make_response_menu(
                    1,
                    TransferKind::SecondOpinion,
                    AnchorSlot::default(),
                    Some(vec![
                        (Kind::Codex, None),
                        (Kind::Claude, None),
                        (Kind::Cursor, None),
                    ]),
                    window,
                    cx,
                );
                assert!(view.response_menu.as_ref().unwrap().requested.is_empty());
            })
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "right");
        cx.run_until_parked();
        window
            .update(cx, |view, window, cx| {
                let menu = view.response_menu.as_ref().unwrap();
                assert_eq!(menu.requested, HashSet::from([Kind::Codex]));
                let root = menu.menu.entity_id();
                let models = menu.models[&Kind::Codex].clone();
                assert!(models.focus_handle(cx).is_focused(window));
                view.finish_response_models(
                    view.response_request,
                    Kind::Codex,
                    &models,
                    Ok(vec![model("review", "Review model")]),
                    cx,
                );
                assert_eq!(view.response_menu.as_ref().unwrap().menu.entity_id(), root);
                assert!(models.focus_handle(cx).is_focused(window));
            })
            .unwrap();
        cx.run_until_parked();
        // The now-loaded model can open its effort submenu without reopening
        // the provider picker or waiting for either other catalog.
        cx.simulate_keystrokes(window.into(), "right down enter");
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| assert!(view.response_menu.is_none()))
            .unwrap();
    }

    #[gpui::test]
    fn response_picker_ignores_old_completions_and_insets_model_rows(cx: &mut TestAppContext) {
        let window = conversation(cx);
        window
            .update(cx, |view, window, cx| {
                let request = Some((1, TransferKind::Handoff, Instant::now()));
                view.response_request = request;
                let anchor = AnchorSlot::default();
                anchor.set(Some(gpui::Bounds::new(
                    gpui::point(px(100.), px(300.)),
                    gpui::size(px(20.), px(20.)),
                )));
                view.make_response_menu(
                    1,
                    TransferKind::Handoff,
                    anchor,
                    Some(vec![(
                        Kind::Codex,
                        Some(Ok(vec![model("review", "Review model")])),
                    )]),
                    window,
                    cx,
                );
                let models = view.response_menu.as_ref().unwrap().models[&Kind::Codex].clone();
                view.finish_response_models(
                    Some((1, TransferKind::Handoff, Instant::now())),
                    Kind::Codex,
                    &models,
                    Err("Stale failure".into()),
                    cx,
                );
            })
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "right");
        cx.run_until_parked();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        let row = visual
            .debug_bounds(
                r#"menu-action-{"provider":"codex","model":"review","effort":null,"settings":{}}"#,
            )
            .expect("the stale failure did not replace the model");
        let surface = window
            .update(visual, |view, _, cx| {
                let models = &view.response_menu.as_ref().unwrap().models[&Kind::Codex];
                models.read(cx).surface_region().get().unwrap()
            })
            .unwrap();
        assert_eq!(row.left() - surface.left(), px(5.));
        assert_eq!(surface.right() - row.right(), px(5.));
        assert_eq!(row.top() - surface.top(), px(5.));
        assert_eq!(surface.bottom() - row.bottom(), px(5.));
    }

    #[gpui::test]
    fn second_opinion_excludes_the_resolved_alias_of_the_current_model(cx: &mut TestAppContext) {
        let window = conversation(cx);
        window
            .update(cx, |view, _, cx| {
                view.transcript.update(cx, |transcript, _| {
                    transcript.turns.insert(
                        1,
                        monocode_core::agent::TurnInfo {
                            provider: "claude".into(),
                            model: "Opus 5.5".into(),
                            model_id: Some("claude:opus-5-5".into()),
                            ..Default::default()
                        },
                    );
                });
                let mut opus = model("opus", "Opus 5.5");
                opus.alias_target = Some("claude-opus-5-5".into());
                let options = view.response_model_items(
                    1,
                    TransferKind::SecondOpinion,
                    Kind::Claude,
                    Ok(vec![opus, model("sonnet", "Sonnet 5")]),
                    cx,
                );
                assert_eq!(options.len(), 1);
                let MenuItem::Action(action) = &options[0] else {
                    panic!("a different model remains");
                };
                assert_eq!(action.label.as_ref(), "Sonnet 5");
                let target: Target = serde_json::from_str(&action.id).unwrap();
                assert_eq!(target.model.as_deref(), Some("sonnet"));
            })
            .unwrap();
    }
}

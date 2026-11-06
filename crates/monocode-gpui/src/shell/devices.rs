//! The Device tab from the shell's side: opening it, keeping it with its
//! project, and the `devices.*` actions agents reach through the MonoCode MCP
//! server.
use super::*;
use crate::right_panel::RightTab;
use monocode_core::{
    device::{self, Device, Platform, hub, toolchain},
    mono::bridge::Call,
};
use serde_json::{Value, json};

/// Screenshots kept for agents to read back; older ones are deleted.
const KEPT_SCREENSHOTS: usize = 20;

/// The right panel is shared, but its Device tab belongs to the project it
/// was opened in.
#[derive(Default)]
pub(super) struct DeviceScope {
    /// The project the panel last showed.
    project: Option<String>,
    /// Projects whose Device tab waits for them, each with its device.
    waiting: HashMap<String, Option<Device>>,
    /// The Device tab opened the panel, so leaving it closes the panel.
    pub(super) opened_panel: bool,
}

impl Shell {
    /// Moves the Device tab out of the way of a project that doesn't have
    /// one, and brings it back with the project that does.
    pub(super) fn sync_device_scope(&mut self, cx: &mut Context<Self>) {
        let project = self.workspace.project.clone();
        if self.device_scope.project.as_ref() == Some(&project) {
            return;
        }
        let left = self.device_scope.project.replace(project.clone());
        let (Some(left), Some(panel)) = (left, self.right_panel.clone()) else {
            return;
        };
        let device_panel = panel.read(cx).device.clone();
        let open = self.workspace.right_panel_open;
        let tab_open = panel.read(cx).tab() == RightTab::Device;
        if tab_open && open {
            let device = device_panel.read(cx).selected().cloned();
            self.device_scope.waiting.insert(left, device);
        } else {
            self.device_scope.waiting.remove(&left);
        }
        match self.device_scope.waiting.get(&project).cloned() {
            Some(device) => {
                if !(tab_open && open) {
                    self.device_scope.opened_panel = !open;
                }
                panel.update(cx, |panel, cx| panel.select(RightTab::Device, cx));
                if let Some(device) = device {
                    device_panel.update(cx, |panel, cx| panel.recall(&device, cx));
                }
                self.slide_right_panel(true, cx);
            }
            None if tab_open => {
                let close = self.device_scope.opened_panel;
                panel.update(cx, |panel, cx| panel.leave_device(cx));
                if close && open {
                    self.slide_right_panel(false, cx);
                }
            }
            None => return,
        }
        self.fit_sidebar(cx);
        self.invalidate_chrome(cx);
        cx.notify();
    }

    /// Shows the Device tab, opening the right panel for it. With devices
    /// off, opens Settings › Devices instead.
    pub(super) fn open_device_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.devices_in_panel() {
            self.open_settings(crate::settings::Section::Devices, window, cx);
            return;
        }
        self.ensure_right_panel(window, cx);
        let Some(panel) = self.right_panel.clone() else {
            return;
        };
        let open = self.workspace.right_panel_open;
        if !open || panel.read(cx).tab() != RightTab::Device {
            self.device_scope.opened_panel = !open;
        }
        panel.update(cx, |panel, cx| panel.select(RightTab::Device, cx));
        self.workspace.right_panel_width = self.workspace.right_panel_width.max(380.);
        if self.workspace.right_panel_open {
            self.sync_right_panel(cx);
        } else {
            self.set_right_panel_open(true, window, cx);
        }
        self.invalidate_chrome(cx);
        cx.notify();
    }

    pub(super) fn toggle_device_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let showing = self.workspace.right_panel_open
            && self
                .right_panel
                .as_ref()
                .is_some_and(|panel| panel.read(cx).tab() == RightTab::Device);
        if showing {
            self.set_right_panel_open(false, window, cx);
        } else {
            self.open_device_panel(window, cx);
        }
    }

    /// A project other than the one on screen. A thread outside every
    /// project, such as a Mono's, has no project to wait for.
    fn device_elsewhere(&self, project: &str) -> bool {
        project != self.workspace.project && self.workspace.projects.iter().any(|p| p == project)
    }

    /// Shows the device an agent opened, or leaves it on `project`'s Device
    /// tab if the user is working in another project.
    fn show_opened_device(
        &mut self,
        project: &str,
        device: Device,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.device_elsewhere(project) {
            self.device_scope
                .waiting
                .insert(project.to_owned(), Some(device));
            return;
        }
        self.open_device_panel(window, cx);
        if let Some(panel) = self.right_panel.clone() {
            panel.update(cx, |panel, cx| {
                panel.device.update(cx, |d, cx| d.show(device, cx))
            });
        }
    }

    /// The Device tab's screenshots and recordings go into the message being
    /// written in the conversation on screen.
    pub(super) fn attach_to_visible_message(
        &mut self,
        paths: Vec<std::path::PathBuf>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chat = self
            .workspace
            .active
            .map(|pane| self.workspace.session_for_pane(pane))
            .and_then(|id| self.sessions.get(&id).cloned());
        match chat {
            Some(chat) => chat.update(cx, |chat, cx| chat.add_attachments(paths, cx)),
            None => self.notification_state.file_error.update(cx, |notice, cx| {
                notice.set_message(
                    Some("Open a thread to attach the screenshot to.".into()),
                    cx,
                )
            }),
        }
        cx.notify();
    }

    fn authorize_device_call(&self, call: &Call, cx: &App) -> Result<(), String> {
        device::validate(&call.action, &call.input)?;
        if !cfg!(target_os = "macos") {
            return Err("Devices need the macOS app.".into());
        }
        if !device::agents_enabled() {
            return Err(
                "Agent access to devices is off. The user can turn it on in Settings › Devices."
                    .into(),
            );
        }
        let caller = self
            .sessions
            .get(&call.session)
            .map(|view| view.read(cx))
            .ok_or("This thread is not open in MonoCode.")?;
        if caller.summary.cwd.starts_with("remote://") {
            return Err("Devices are not available on remote projects.".into());
        }
        let read_only = caller.app_read_only
            || caller.settings.plan
            || caller.settings.access == monocode_core::agent::AccessMode::ReadOnly;
        if read_only && !matches!(call.action.as_str(), "devices.list" | "devices.screenshot") {
            return Err(
                "This thread's access policy allows only device_list and device_screenshot.".into(),
            );
        }
        Ok(())
    }

    pub(super) fn handle_device_call(
        &mut self,
        call: Call,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.authorize_device_call(&call, cx) {
            let _ = call.reply.send(Err(error));
            return;
        }
        let query = call.input["device"].as_str().map(str::to_owned);
        let platform = match call.input["platform"].as_str() {
            Some("ios") => Some(Platform::Ios),
            Some("android") => Some(Platform::Android),
            _ => None,
        };
        let project = self
            .sessions
            .get(&call.session)
            .map(|view| view.read(cx).summary.cwd.clone())
            .unwrap_or_default();
        // The device on the caller's Device tab, for a screenshot that names none.
        let shown = if self.device_elsewhere(&project) {
            self.device_scope.waiting.get(&project).cloned().flatten()
        } else {
            self.right_panel
                .as_ref()
                .and_then(|panel| panel.read(cx).device.read(cx).selected().cloned())
        };
        let session = call.session;
        let action = call.action.clone();
        let setting = call.input["setting"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let value = call.input["value"].as_str().unwrap_or_default().to_owned();
        let work = cx.background_executor().spawn(async move {
            let find = |query: &str| -> Result<Device, String> {
                let list = hub::list()?;
                device::resolve(&list, query, platform)
                    .cloned()
                    .ok_or_else(|| {
                        let names: Vec<String> = list
                            .devices
                            .iter()
                            .filter(|d| d.supported)
                            .map(|d| format!("{} ({})", d.name, d.id))
                            .collect();
                        format!("No device matches {query:?}. Devices: {}", names.join(", "))
                    })
            };
            match action.as_str() {
                "devices.list" => {
                    let list = hub::list()?;
                    Ok((json!(list), None))
                }
                "devices.open" => {
                    let mut device = find(query.as_deref().unwrap_or_default())?;
                    if !device.booted {
                        device.id = hub::boot(&device)?;
                        device.booted = true;
                    }
                    toolchain::install(toolchain::Tool::AgentDevice)?;
                    let launcher = toolchain::agent_launcher()?;
                    let name = device::session_name(session, &device.id);
                    Ok((device::quick_start(&device, &launcher, &name), Some(device)))
                }
                "devices.screenshot" => {
                    let device = match (query.as_deref(), shown) {
                        (Some(query), _) => find(query)?,
                        (None, Some(device)) => device,
                        (None, None) => {
                            return Err(
                                "No device is open. Call device_open first, or name one.".into()
                            );
                        }
                    };
                    if !device.booted {
                        return Err(format!(
                            "{} is not running. Call device_open first.",
                            device.name
                        ));
                    }
                    let png = hub::screenshot(device.platform, &device.id)?;
                    let path = save_screenshot(session, &png)?;
                    Ok((json!({"device": device, "path": path}), None))
                }
                "devices.set" => {
                    let device = match (query.as_deref(), shown) {
                        (Some(query), _) => find(query)?,
                        (None, Some(device)) => device,
                        (None, None) => {
                            return Err(
                                "No device is open. Call device_open first, or name one.".into()
                            );
                        }
                    };
                    if device.platform != Platform::Ios {
                        return Err("device_set changes iOS Simulators only.".into());
                    }
                    if !device.booted {
                        return Err(format!(
                            "{} is not running. Call device_open first.",
                            device.name
                        ));
                    }
                    let applied = match agent_change(&setting, &value)? {
                        AgentChange::Setting(setting, value) => {
                            crate::device::sim::set(&device.id, setting, value)?;
                            value.to_owned()
                        }
                        AgentChange::Place(at) => {
                            crate::device::sim::set_location(&*hub::ensure()?, &device.id, at)?;
                            at.map(|(lat, lng)| format!("{lat},{lng}"))
                                .unwrap_or_else(|| "clear".into())
                        }
                    };
                    Ok((
                        json!({"device": device.id, "setting": setting, "value": applied}),
                        None,
                    ))
                }
                "devices.close" => {
                    let device = find(query.as_deref().unwrap_or_default())?;
                    if device.booted {
                        hub::shutdown(&device)?;
                    }
                    Ok((json!({"device": device.id, "shutDown": true}), None))
                }
                _ => Err("Unknown device action".into()),
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result: Result<(Value, Option<Device>), String> = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if let Ok((_, Some(device))) = &result {
                    this.show_opened_device(&project, device.clone(), window, cx);
                }
                if call.action == "devices.set"
                    && let Ok((value, _)) = &result
                    && let Some(panel) = this.right_panel.clone()
                    && let Ok(change) = agent_change(
                        value["setting"].as_str().unwrap_or_default(),
                        value["value"].as_str().unwrap_or_default(),
                    )
                {
                    let id = value["device"].as_str().unwrap_or_default().to_owned();
                    panel.update(cx, |panel, cx| {
                        panel.device.update(cx, |d, cx| {
                            if d.selected().is_some_and(|shown| shown.id == id) {
                                match change {
                                    AgentChange::Setting(setting, value) => {
                                        d.agent_set(Some((setting, value)), None, cx)
                                    }
                                    AgentChange::Place(at) => d.agent_set(
                                        None,
                                        Some(at.map(|(lat, lng)| {
                                            (format!("{lat:.3}, {lng:.3}").into(), lat, lng)
                                        })),
                                        cx,
                                    ),
                                }
                            }
                        })
                    });
                }
                if call.action == "devices.close"
                    && result.is_ok()
                    && let Some(panel) = this.right_panel.clone()
                {
                    panel.update(cx, |panel, cx| {
                        panel.device.update(cx, |d, cx| d.refresh(cx))
                    });
                }
                let _ = call.reply.send(result.map(|(value, _)| value));
            });
        })
        .detach();
    }
}

/// What an agent's `device_set` asks for.
enum AgentChange {
    Setting(crate::device::sim::Setting, &'static str),
    Place(Option<(f64, f64)>),
}

fn agent_change(setting: &str, value: &str) -> Result<AgentChange, String> {
    use crate::device::sim;
    if setting == "location" {
        return if matches!(value.trim(), "clear" | "off" | "none") {
            Ok(AgentChange::Place(None))
        } else {
            sim::parse_coordinates(value)
                .map(|at| AgentChange::Place(Some(at)))
                .ok_or_else(|| "location takes \"latitude,longitude\" or clear".to_string())
        };
    }
    let setting = sim::Setting::from_key(setting).ok_or("Unknown setting")?;
    Ok(AgentChange::Setting(setting, setting.accept(value)?))
}

fn save_screenshot(session: i64, png: &[u8]) -> Result<String, String> {
    let dir = toolchain::root().join("screenshots");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{session}-{}.png", monocode_core::time::now_ms()));
    std::fs::write(&path, png).map_err(|e| e.to_string())?;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut files: Vec<_> = entries
            .flatten()
            .filter_map(|entry| {
                let modified = entry.metadata().ok()?.modified().ok()?;
                Some((modified, entry.path()))
            })
            .collect();
        if files.len() > KEPT_SCREENSHOTS {
            files.sort();
            for (_, old) in &files[..files.len() - KEPT_SCREENSHOTS] {
                let _ = std::fs::remove_file(old);
            }
        }
    }
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use gpui::{TestAppContext, WindowHandle};

    const HOME: &str = "/work/monocode";
    const OTHER: &str = "/work/another-project";

    fn device(name: &str) -> Device {
        Device {
            id: name.into(),
            name: name.into(),
            version: String::new(),
            platform: Platform::Ios,
            booted: false,
            physical: false,
            supported: true,
        }
    }

    fn setup(cx: &mut TestAppContext) -> WindowHandle<Shell> {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, _, _) = super::super::tests::window(cx);
        window
            .update(cx, |shell, window, cx| {
                shell.workspace.preferences.devices = true;
                shell.ensure_right_panel(window, cx);
                shell.git.update(cx, |git, _| {
                    git.stats = Some(monocode_core::scm::DiffStats {
                        files: 1,
                        additions: 1,
                        deletions: 0,
                    });
                });
                shell.sync_right_panel(cx);
            })
            .unwrap();
        window
    }

    fn panel_state(shell: &Shell, cx: &App) -> (RightTab, bool) {
        let panel = shell.right_panel.as_ref().unwrap().read(cx);
        (panel.tab(), shell.workspace.right_panel_open)
    }

    fn shown_device(shell: &Shell, cx: &App) -> Option<String> {
        let panel = shell.right_panel.as_ref().unwrap().read(cx);
        panel.device.read(cx).selected().map(|d| d.name.clone())
    }

    fn pick(shell: &mut Shell, tab: RightTab, cx: &mut Context<Shell>) {
        let panel = shell.right_panel.clone().unwrap();
        panel.update(cx, |panel, cx| panel.select(tab, cx));
    }

    fn pick_device(shell: &mut Shell, name: &str, cx: &mut Context<Shell>) {
        let device_panel = shell.right_panel.as_ref().unwrap().read(cx).device.clone();
        device_panel.update(cx, |panel, cx| panel.recall(&device(name), cx));
    }

    #[gpui::test]
    fn device_tab_stays_with_the_project_it_opened_in(cx: &mut TestAppContext) {
        let window = setup(cx);
        window
            .update(cx, |shell, window, cx| {
                shell.set_right_panel_open(true, window, cx);
                pick(shell, RightTab::Changes, cx);
                shell.open_device_panel(window, cx);
                shell.select_project(OTHER.into(), window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, window, cx| {
                assert_eq!(panel_state(shell, cx), (RightTab::Changes, true));
                shell.select_project(HOME.into(), window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .read_with(cx, |shell, cx| {
                assert_eq!(panel_state(shell, cx), (RightTab::Device, true));
            })
            .unwrap();
    }

    #[gpui::test]
    fn a_panel_the_device_tab_opened_closes_in_other_projects(cx: &mut TestAppContext) {
        let window = setup(cx);
        window
            .update(cx, |shell, window, cx| {
                assert!(!shell.workspace.right_panel_open);
                shell.open_device_panel(window, cx);
                assert!(shell.workspace.right_panel_open);
                shell.select_project(OTHER.into(), window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, window, cx| {
                assert_eq!(panel_state(shell, cx), (RightTab::Files, false));
                shell.select_project(HOME.into(), window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .read_with(cx, |shell, cx| {
                assert_eq!(panel_state(shell, cx), (RightTab::Device, true));
            })
            .unwrap();
    }

    #[gpui::test]
    fn each_project_goes_back_to_its_own_device(cx: &mut TestAppContext) {
        let window = setup(cx);
        window
            .update(cx, |shell, window, cx| {
                shell.open_device_panel(window, cx);
                pick_device(shell, "iPhone", cx);
                shell.select_project(OTHER.into(), window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, window, cx| {
                shell.open_device_panel(window, cx);
                pick_device(shell, "Pixel", cx);
                shell.select_project(HOME.into(), window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, window, cx| {
                assert_eq!(panel_state(shell, cx), (RightTab::Device, true));
                assert_eq!(shown_device(shell, cx).as_deref(), Some("iPhone"));
                shell.select_project(OTHER.into(), window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .read_with(cx, |shell, cx| {
                assert_eq!(panel_state(shell, cx), (RightTab::Device, true));
                assert_eq!(shown_device(shell, cx).as_deref(), Some("Pixel"));
            })
            .unwrap();
    }

    #[gpui::test]
    fn leaving_the_device_tab_before_switching_lets_it_go(cx: &mut TestAppContext) {
        let window = setup(cx);
        window
            .update(cx, |shell, window, cx| {
                shell.open_device_panel(window, cx);
                pick(shell, RightTab::Changes, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, window, cx| {
                shell.select_project(OTHER.into(), window, cx);
                shell.select_project(HOME.into(), window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .read_with(cx, |shell, cx| {
                assert_eq!(panel_state(shell, cx), (RightTab::Changes, true));
            })
            .unwrap();
    }

    #[gpui::test]
    fn a_device_tab_closed_by_hand_stays_closed(cx: &mut TestAppContext) {
        let window = setup(cx);
        window
            .update(cx, |shell, window, cx| {
                shell.open_device_panel(window, cx);
                shell.toggle_device_panel(window, cx);
                assert!(!shell.workspace.right_panel_open);
                shell.select_project(OTHER.into(), window, cx);
                shell.select_project(HOME.into(), window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .read_with(cx, |shell, cx| {
                assert_eq!(panel_state(shell, cx), (RightTab::Files, false));
            })
            .unwrap();
    }

    #[gpui::test]
    fn an_agent_in_another_project_leaves_the_screen_alone(cx: &mut TestAppContext) {
        let window = setup(cx);
        window
            .update(cx, |shell, window, cx| {
                shell.show_opened_device(OTHER, device("iPhone"), window, cx);
                assert_eq!(panel_state(shell, cx), (RightTab::Files, false));
                assert_eq!(shown_device(shell, cx), None);
                shell.select_project(OTHER.into(), window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .read_with(cx, |shell, cx| {
                assert_eq!(panel_state(shell, cx), (RightTab::Device, true));
                assert_eq!(shown_device(shell, cx).as_deref(), Some("iPhone"));
            })
            .unwrap();
    }
}
